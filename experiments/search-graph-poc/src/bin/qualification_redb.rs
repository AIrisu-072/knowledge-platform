//! Native parallel redb adapter for the isolated P3-P04 qualification.
//! Stage/reader/writer use one shared file. The prior correctness adapter stays unchanged.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde_json::{Value, json};

const RESOURCES: TableDefinition<&str, &[u8]> = TableDefinition::new("refinement_resources");
const RELATIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("refinement_relations");
const PARTICIPANTS: TableDefinition<&str, &[u8]> = TableDefinition::new("refinement_participants");
const INCIDENCE: TableDefinition<&str, &str> = TableDefinition::new("refinement_incidence");
// Independent resource-first membership lets a read detect a missing reverse
// incidence key even when the missing key was the query seed.
const MEMBERSHIP: TableDefinition<&str, &str> = TableDefinition::new("qualification_membership");
const GENERATIONS: TableDefinition<&str, &str> = TableDefinition::new("refinement_generations");
const SOURCE_POLICIES: TableDefinition<&str, &[u8]> =
    TableDefinition::new("qualification_source_policies");
const SOURCE_REVISIONS: TableDefinition<&str, &str> =
    TableDefinition::new("qualification_source_revisions");
static ACTIVE_FRONTIER_READERS: AtomicU64 = AtomicU64::new(0);
static MAX_FRONTIER_READERS: AtomicU64 = AtomicU64::new(0);

struct FrontierReadGuard;

impl FrontierReadGuard {
    fn enter() -> Self {
        let active = ACTIVE_FRONTIER_READERS.fetch_add(1, Ordering::AcqRel) + 1;
        MAX_FRONTIER_READERS.fetch_max(active, Ordering::AcqRel);
        Self
    }
}

impl Drop for FrontierReadGuard {
    fn drop(&mut self) {
        ACTIVE_FRONTIER_READERS.fetch_sub(1, Ordering::AcqRel);
    }
}

fn key(source: &str, generation: &str, suffix: &str) -> String {
    format!("{source}|{generation}|{suffix}")
}

fn prefix(source: &str, generation: &str) -> String {
    key(source, generation, "")
}

fn bound(prefix: &str) -> String {
    format!("{prefix}\u{10ffff}")
}

fn as_str<'a>(value: &'a Value, name: &str) -> &'a str {
    value[name]
        .as_str()
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn insert_relation(
    relations: &mut redb::Table<'_, &str, &[u8]>,
    participants: &mut redb::Table<'_, &str, &[u8]>,
    incidence: &mut redb::Table<'_, &str, &str>,
    membership: &mut redb::Table<'_, &str, &str>,
    source: &str,
    generation: &str,
    relation: &Value,
) {
    let rid = as_str(relation, "relation_id");
    let rk = key(source, generation, rid);
    let bytes = serde_json::to_vec(relation).unwrap();
    relations.insert(rk.as_str(), bytes.as_slice()).unwrap();
    for (ordinal, member) in relation["participants"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let role = as_str(member, "role");
        let resource = as_str(member, "resource_ref");
        let pk = key(source, generation, &format!("{rid}|{ordinal:08}"));
        let pb = serde_json::to_vec(&json!([ordinal, role, resource])).unwrap();
        participants.insert(pk.as_str(), pb.as_slice()).unwrap();
        let ik = key(
            source,
            generation,
            &format!("{resource}|{role}|{rid}|{ordinal:08}"),
        );
        incidence.insert(ik.as_str(), "1").unwrap();
        membership.insert(ik.as_str(), "1").unwrap();
    }
}

fn remove_relation(
    relations: &mut redb::Table<'_, &str, &[u8]>,
    participants: &mut redb::Table<'_, &str, &[u8]>,
    incidence: &mut redb::Table<'_, &str, &str>,
    membership: &mut redb::Table<'_, &str, &str>,
    source: &str,
    generation: &str,
    relation: &Value,
) {
    let rid = as_str(relation, "relation_id");
    let rk = key(source, generation, rid);
    relations.remove(rk.as_str()).unwrap();
    for (ordinal, member) in relation["participants"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let role = as_str(member, "role");
        let resource = as_str(member, "resource_ref");
        let pk = key(source, generation, &format!("{rid}|{ordinal:08}"));
        participants.remove(pk.as_str()).unwrap();
        let ik = key(
            source,
            generation,
            &format!("{resource}|{role}|{rid}|{ordinal:08}"),
        );
        incidence.remove(ik.as_str()).unwrap();
        membership.remove(ik.as_str()).unwrap();
    }
}

fn stage_full(db: &Database, generation: &Value, override_id: Option<&str>) {
    let source = as_str(generation, "source_id");
    let gid = override_id.unwrap_or_else(|| as_str(generation, "generation_id"));
    let tx = db.begin_write().unwrap();
    {
        let mut resources = tx.open_table(RESOURCES).unwrap();
        let mut relations = tx.open_table(RELATIONS).unwrap();
        let mut participants = tx.open_table(PARTICIPANTS).unwrap();
        let mut incidence = tx.open_table(INCIDENCE).unwrap();
        let mut membership = tx.open_table(MEMBERSHIP).unwrap();
        let mut generations = tx.open_table(GENERATIONS).unwrap();
        for row in generation["resources"].as_array().unwrap() {
            let mut saved = row.clone();
            saved["generation_id"] = json!(gid);
            let rk = key(source, gid, as_str(&saved, "resource_id"));
            let bytes = serde_json::to_vec(&saved).unwrap();
            resources.insert(rk.as_str(), bytes.as_slice()).unwrap();
        }
        for relation in generation["relations"].as_array().unwrap() {
            insert_relation(
                &mut relations,
                &mut participants,
                &mut incidence,
                &mut membership,
                source,
                gid,
                relation,
            );
        }
        let gk = key(source, gid, "state");
        generations.insert(gk.as_str(), "READY").unwrap();
    }
    tx.commit().unwrap();
}

fn bytes_at(
    db: &Database,
    definition: TableDefinition<&str, &[u8]>,
    p: &str,
) -> Vec<(String, Vec<u8>)> {
    let tx = db.begin_read().unwrap();
    let table = tx.open_table(definition).unwrap();
    table
        .range(p..bound(p).as_str())
        .unwrap()
        .map(|item| {
            let (k, v) = item.unwrap();
            (k.value().to_owned(), v.value().to_vec())
        })
        .collect()
}

fn incidence_at(db: &Database, p: &str) -> Vec<String> {
    let tx = db.begin_read().unwrap();
    let table = tx.open_table(INCIDENCE).unwrap();
    table
        .range(p..bound(p).as_str())
        .unwrap()
        .map(|item| item.unwrap().0.value().to_owned())
        .collect()
}

fn membership_at(db: &Database, p: &str) -> Vec<String> {
    let tx = db.begin_read().unwrap();
    let table = tx.open_table(MEMBERSHIP).unwrap();
    table
        .range(p..bound(p).as_str())
        .unwrap()
        .map(|item| item.unwrap().0.value().to_owned())
        .collect()
}

fn stage_incremental(db: &Database, baseline: &Value, updated: &Value) {
    let source = as_str(updated, "source_id");
    let base = as_str(baseline, "generation_id");
    let target = as_str(updated, "generation_id");
    let from = prefix(source, base);
    let to = prefix(source, target);
    let resources_copy = bytes_at(db, RESOURCES, &from);
    let relations_copy = bytes_at(db, RELATIONS, &from);
    let participants_copy = bytes_at(db, PARTICIPANTS, &from);
    let incidence_copy = incidence_at(db, &from);
    let membership_copy = membership_at(db, &from);
    let old: BTreeMap<&str, &Value> = baseline["relations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (as_str(r, "relation_id"), r))
        .collect();
    let new: BTreeMap<&str, &Value> = updated["relations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (as_str(r, "relation_id"), r))
        .collect();
    let tx = db.begin_write().unwrap();
    {
        let mut resources = tx.open_table(RESOURCES).unwrap();
        let mut relations = tx.open_table(RELATIONS).unwrap();
        let mut participants = tx.open_table(PARTICIPANTS).unwrap();
        let mut incidence = tx.open_table(INCIDENCE).unwrap();
        let mut membership = tx.open_table(MEMBERSHIP).unwrap();
        let mut generations = tx.open_table(GENERATIONS).unwrap();
        for (k, raw) in resources_copy {
            let mut value: Value = serde_json::from_slice(&raw).unwrap();
            value["generation_id"] = json!(target);
            let bytes = serde_json::to_vec(&value).unwrap();
            let target_key = k.replacen(&from, &to, 1);
            resources
                .insert(target_key.as_str(), bytes.as_slice())
                .unwrap();
        }
        for (k, bytes) in relations_copy {
            let target_key = k.replacen(&from, &to, 1);
            relations
                .insert(target_key.as_str(), bytes.as_slice())
                .unwrap();
        }
        for (k, bytes) in participants_copy {
            let target_key = k.replacen(&from, &to, 1);
            participants
                .insert(target_key.as_str(), bytes.as_slice())
                .unwrap();
        }
        for k in incidence_copy {
            let target_key = k.replacen(&from, &to, 1);
            incidence.insert(target_key.as_str(), "1").unwrap();
        }
        for k in membership_copy {
            let target_key = k.replacen(&from, &to, 1);
            membership.insert(target_key.as_str(), "1").unwrap();
        }
        for (rid, prior) in &old {
            if new.get(rid) != Some(prior) {
                remove_relation(
                    &mut relations,
                    &mut participants,
                    &mut incidence,
                    &mut membership,
                    source,
                    target,
                    prior,
                );
            }
        }
        for (rid, replacement) in &new {
            if old.get(rid) != Some(replacement) {
                insert_relation(
                    &mut relations,
                    &mut participants,
                    &mut incidence,
                    &mut membership,
                    source,
                    target,
                    replacement,
                );
            }
        }
        let gk = key(source, target, "state");
        generations.insert(gk.as_str(), "READY").unwrap();
    }
    tx.commit().unwrap();
}

fn stage(fixture_path: &str, db_path: &str) {
    let fixture: Value = serde_json::from_slice(&fs::read(fixture_path).unwrap()).unwrap();
    assert!(
        !std::path::Path::new(db_path).exists(),
        "refuse to overwrite existing redb file"
    );
    let db = Database::create(db_path).unwrap();
    let generations = fixture["generations"].as_array().unwrap();
    let start = Instant::now();
    stage_full(&db, &generations[0], None);
    let baseline_full_ns = start.elapsed().as_nanos();
    let start = Instant::now();
    stage_incremental(&db, &generations[0], &generations[1]);
    let updated_incremental_ns = start.elapsed().as_nanos();
    let start = Instant::now();
    stage_full(
        &db,
        &generations[1],
        Some("00000000-0000-0000-0000-000000000066"),
    );
    let updated_full_ns = start.elapsed().as_nanos();
    let start = Instant::now();
    stage_full(&db, &generations[2], None);
    let cross_source_full_ns = start.elapsed().as_nanos();
    let mut writer = generations[0].clone();
    writer["generation_id"] = json!("00000000-0000-0000-0000-0000000000c8");
    let relation = writer["relations"][0].clone();
    let members: BTreeSet<&str> = relation["participants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| as_str(p, "resource_ref"))
        .collect();
    writer["resources"] = json!(
        writer["resources"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| members.contains(as_str(row, "resource_id")))
            .cloned()
            .collect::<Vec<_>>()
    );
    writer["relations"] = json!([relation]);
    stage_full(&db, &writer, None);
    let tx = db.begin_write().unwrap();
    {
        let mut generations = tx.open_table(GENERATIONS).unwrap();
        let state = key(
            as_str(&writer, "source_id"),
            as_str(&writer, "generation_id"),
            "state",
        );
        generations.insert(state.as_str(), "BUILDING").unwrap();
    }
    tx.commit().unwrap();
    let source_start = Instant::now();
    let tx = db.begin_write().unwrap();
    {
        let mut policies = tx.open_table(SOURCE_POLICIES).unwrap();
        let mut revisions = tx.open_table(SOURCE_REVISIONS).unwrap();
        for (source, entries) in fixture["authority"].as_object().unwrap() {
            for (resource, policy) in entries.as_object().unwrap() {
                let k = format!("{source}|{resource}");
                let raw = serde_json::to_vec(policy).unwrap();
                policies.insert(k.as_str(), raw.as_slice()).unwrap();
            }
        }
        for scenario in fixture["scenarios"].as_array().unwrap() {
            let k = format!(
                "{}|{}",
                as_str(scenario, "source_id"),
                as_str(scenario, "name")
            );
            revisions
                .insert(k.as_str(), as_str(scenario, "revision"))
                .unwrap();
        }
    }
    tx.commit().unwrap();
    let source_authority_ns = source_start.elapsed().as_nanos();
    println!(
        "{}",
        json!({"status":"ready", "transport":"redb 4.3.0 native transaction",
            "baseline_full_ns":baseline_full_ns, "updated_incremental_ns":updated_incremental_ns,
            "updated_full_ns":updated_full_ns, "cross_source_full_ns":cross_source_full_ns,
            "source_authority_ns":source_authority_ns,
            "file_bytes":fs::metadata(db_path).unwrap().len()})
    );
}

fn read_value(db: &Database, definition: TableDefinition<&str, &[u8]>, k: &str) -> Value {
    let tx = db.begin_read().unwrap();
    let table = tx.open_table(definition).unwrap();
    table
        .get(k)
        .unwrap()
        .map(|v| serde_json::from_slice(v.value()).unwrap())
        .unwrap_or(Value::Null)
}

fn ready(db: &Database, source: &str, generation: &str) -> bool {
    let tx = db.begin_read().unwrap();
    let table = tx.open_table(GENERATIONS).unwrap();
    let k = key(source, generation, "state");
    table
        .get(k.as_str())
        .unwrap()
        .is_some_and(|v| v.value() == "READY")
}

fn frontier(db: &Database, source: &str, generation: &str, seeds: &[Value], role: &str) -> Value {
    let _read_guard = FrontierReadGuard::enter();
    let tx = db.begin_read().unwrap();
    let incidence = tx.open_table(INCIDENCE).unwrap();
    let membership = tx.open_table(MEMBERSHIP).unwrap();
    let relations = tx.open_table(RELATIONS).unwrap();
    let participants = tx.open_table(PARTICIPANTS).unwrap();
    let resources = tx.open_table(RESOURCES).unwrap();
    let mut ids = BTreeSet::new();
    for seed in seeds {
        let resource = seed.as_str().expect("frontier seed must be an ID");
        let seed_key = key(source, generation, &format!("{resource}|{role}|"));
        let reverse: BTreeSet<String> = incidence
            .range(seed_key.as_str()..bound(&seed_key).as_str())
            .unwrap()
            .map(|row| row.unwrap().0.value().to_owned())
            .collect();
        let forward: BTreeSet<String> = membership
            .range(seed_key.as_str()..bound(&seed_key).as_str())
            .unwrap()
            .map(|row| row.unwrap().0.value().to_owned())
            .collect();
        assert_eq!(reverse, forward, "missing or surplus reverse incidence key");
        for incidence_key in reverse {
            let parts: Vec<&str> = incidence_key.split('|').collect();
            assert_eq!(parts.len(), 6, "malformed incidence key");
            assert_eq!(parts[2], resource);
            assert_eq!(parts[3], role);
            ids.insert(parts[4].to_owned());
        }
    }
    let mut packet = Vec::with_capacity(ids.len());
    for rid in ids {
        let relation_key = key(source, generation, &rid);
        let payload: Value = serde_json::from_slice(
            relations
                .get(relation_key.as_str())
                .unwrap()
                .expect("incidence points at absent relation")
                .value(),
        )
        .unwrap();
        assert_eq!(as_str(&payload, "relation_id"), rid);
        let participant_prefix = key(source, generation, &format!("{rid}|"));
        let stored: Vec<Value> = participants
            .range(participant_prefix.as_str()..bound(&participant_prefix).as_str())
            .unwrap()
            .map(|row| {
                let (k, v) = row.unwrap();
                let member: Value = serde_json::from_slice(v.value()).unwrap();
                let ordinal = member[0].as_u64().expect("participant ordinal");
                assert_eq!(
                    k.value(),
                    key(source, generation, &format!("{rid}|{ordinal:08}")),
                    "participant key/ordinal mismatch"
                );
                let member_role = member[1].as_str().expect("participant role");
                let member_resource = member[2].as_str().expect("participant resource");
                let reverse_key = key(
                    source,
                    generation,
                    &format!("{member_resource}|{member_role}|{rid}|{ordinal:08}"),
                );
                assert!(
                    incidence.get(reverse_key.as_str()).unwrap().is_some(),
                    "missing reverse incidence key"
                );
                assert!(
                    membership.get(reverse_key.as_str()).unwrap().is_some(),
                    "missing forward membership key"
                );
                assert!(
                    resources
                        .get(key(source, generation, member_resource).as_str())
                        .unwrap()
                        .is_some(),
                    "participant points at absent resource"
                );
                member
            })
            .collect();
        let logical = payload["participants"]
            .as_array()
            .expect("relation participants");
        assert_eq!(
            stored.len(),
            logical.len(),
            "missing native participant row"
        );
        for (ordinal, member) in stored.iter().enumerate() {
            assert_eq!(member[0].as_u64(), Some(ordinal as u64));
            assert_eq!(member[1].as_str(), logical[ordinal]["role"].as_str());
            assert_eq!(
                member[2].as_str(),
                logical[ordinal]["resource_ref"].as_str()
            );
        }
        packet.push(json!([payload, stored]));
    }
    json!(packet)
}

fn bulk_rows(db: &Database, source: &str, generation: &str) -> Value {
    let p = prefix(source, generation);
    let resource_rows: Vec<Value> = bytes_at(db, RESOURCES, &p)
        .into_iter()
        .map(|(_, bytes)| serde_json::from_slice(&bytes).unwrap())
        .collect();
    let relation_rows: Vec<Value> = bytes_at(db, RELATIONS, &p)
        .into_iter()
        .map(|(_, bytes)| serde_json::from_slice(&bytes).unwrap())
        .collect();
    let incidence_keys = incidence_at(db, &p);
    let membership_keys = membership_at(db, &p);
    assert_eq!(
        incidence_keys, membership_keys,
        "native reverse/membership set mismatch"
    );
    let incidence_rows: Vec<Value> = incidence_keys
        .iter()
        .map(|k| {
            let parts: Vec<&str> = k.split('|').collect();
            assert_eq!(parts.len(), 6, "malformed persisted incidence key");
            json!([
                parts[4],
                parts[5].parse::<u64>().unwrap(),
                parts[3],
                parts[2],
                parts[0],
                parts[1]
            ])
        })
        .collect();
    json!([resource_rows, relation_rows, incidence_rows])
}

fn authority_decision(db: &Database, source: &str, scenario: &str, resource: &str) -> Value {
    let tx = db.begin_read().unwrap();
    let policies = tx.open_table(SOURCE_POLICIES).unwrap();
    let revisions = tx.open_table(SOURCE_REVISIONS).unwrap();
    let revision_key = format!("{source}|{scenario}");
    let revision = revisions
        .get(revision_key.as_str())
        .unwrap()
        .expect("Source revision snapshot missing")
        .value()
        .to_owned();
    let policy_key = format!("{source}|{resource}");
    let policy: Value = serde_json::from_slice(
        policies
            .get(policy_key.as_str())
            .unwrap()
            .expect("Source policy missing")
            .value(),
    )
    .unwrap();
    let status = policy["revision"][&revision].as_str().unwrap_or("Unknown");
    json!([revision, policy["mapping"], status])
}

fn source_bulk(db: &Database) -> Value {
    let tx = db.begin_read().unwrap();
    let policies = tx.open_table(SOURCE_POLICIES).unwrap();
    let revisions = tx.open_table(SOURCE_REVISIONS).unwrap();
    let policy_rows: Vec<Value> = policies
        .iter()
        .unwrap()
        .map(|row| {
            let (key, raw) = row.unwrap();
            let (source, resource) = key.value().split_once('|').unwrap();
            let policy: Value = serde_json::from_slice(raw.value()).unwrap();
            json!([source, resource, policy])
        })
        .collect();
    let revision_rows: Vec<Value> = revisions
        .iter()
        .unwrap()
        .map(|row| {
            let (key, value) = row.unwrap();
            let (source, scenario) = key.value().split_once('|').unwrap();
            json!([source, scenario, value.value()])
        })
        .collect();
    json!([policy_rows, revision_rows])
}

fn execute(db: &Database, request: &Value) -> Value {
    let command = as_str(request, "cmd");
    if command == "source_bulk" {
        return source_bulk(db);
    }
    let source = as_str(request, "source");
    if command == "corrupt_source_status" {
        let resource = as_str(request, "resource");
        let revision = as_str(request, "revision");
        let status = as_str(request, "status");
        let policy_key = format!("{source}|{resource}");
        let mut policy = read_value(db, SOURCE_POLICIES, &policy_key);
        assert!(!policy.is_null(), "Source policy missing");
        policy["revision"][revision] = json!(status);
        let raw = serde_json::to_vec(&policy).unwrap();
        let tx = db.begin_write().unwrap();
        {
            let mut policies = tx.open_table(SOURCE_POLICIES).unwrap();
            policies
                .insert(policy_key.as_str(), raw.as_slice())
                .unwrap();
        }
        tx.commit().unwrap();
        return json!(true);
    }
    if command == "authority" {
        return authority_decision(
            db,
            source,
            as_str(request, "scenario"),
            as_str(request, "resource"),
        );
    }
    let generation = as_str(request, "generation");
    if !ready(db, source, generation) {
        return Value::Null;
    }
    match command {
        "frontier" => frontier(
            db,
            source,
            generation,
            request["resources"].as_array().expect("frontier resources"),
            as_str(request, "role"),
        ),
        "bulk_rows" => bulk_rows(db, source, generation),
        "resource" => {
            let rid = as_str(request, "resource");
            read_value(db, RESOURCES, &key(source, generation, rid))
        }
        "neighbors" => {
            let rid = as_str(request, "resource");
            let role = as_str(request, "role");
            let p = key(source, generation, &format!("{rid}|{role}|"));
            let ids: Vec<String> = incidence_at(db, &p)
                .iter()
                .map(|k| k.split('|').nth(4).unwrap().to_owned())
                .collect();
            json!(ids)
        }
        "relation" => {
            let rid = as_str(request, "relation");
            let payload = read_value(db, RELATIONS, &key(source, generation, rid));
            if payload.is_null() {
                Value::Null
            } else {
                let p = key(source, generation, &format!("{rid}|"));
                let participants: Vec<Value> = bytes_at(db, PARTICIPANTS, &p)
                    .iter()
                    .map(|(_, b)| serde_json::from_slice(b).unwrap())
                    .collect();
                json!([payload, participants])
            }
        }
        "all_ids" => {
            let p = prefix(source, generation);
            let resources: Vec<String> = bytes_at(db, RESOURCES, &p)
                .iter()
                .map(|(k, _)| k.rsplit('|').next().unwrap().to_owned())
                .collect();
            let relations: Vec<String> = bytes_at(db, RELATIONS, &p)
                .iter()
                .map(|(k, _)| k.rsplit('|').next().unwrap().to_owned())
                .collect();
            json!([resources, relations])
        }
        "incidence_rows" => {
            let rows: Vec<Value> = incidence_at(db, &prefix(source, generation))
                .iter()
                .map(|k| {
                    let parts: Vec<&str> = k.split('|').collect();
                    assert_eq!(parts.len(), 6, "malformed persisted incidence key");
                    json!([
                        parts[4],
                        parts[5].parse::<u64>().unwrap(),
                        parts[3],
                        parts[2],
                        parts[0],
                        parts[1]
                    ])
                })
                .collect();
            json!(rows)
        }
        "corrupt_participant" => {
            let rid = as_str(request, "relation");
            let ordinal = request["ordinal"].as_u64().unwrap();
            let pk = key(source, generation, &format!("{rid}|{ordinal:08}"));
            let tx = db.begin_write().unwrap();
            {
                let mut participants = tx.open_table(PARTICIPANTS).unwrap();
                participants.remove(pk.as_str()).unwrap();
            }
            tx.commit().unwrap();
            json!(true)
        }
        "corrupt_reverse_incidence" => {
            let rid = as_str(request, "relation");
            let ordinal = request["ordinal"].as_u64().unwrap();
            let pk = key(source, generation, &format!("{rid}|{ordinal:08}"));
            let member = read_value(db, PARTICIPANTS, &pk);
            let resource = member[2].as_str().unwrap();
            let role = member[1].as_str().unwrap();
            let ik = key(
                source,
                generation,
                &format!("{resource}|{role}|{rid}|{ordinal:08}"),
            );
            let tx = db.begin_write().unwrap();
            {
                let mut incidence = tx.open_table(INCIDENCE).unwrap();
                incidence.remove(ik.as_str()).unwrap();
            }
            tx.commit().unwrap();
            json!(true)
        }
        "corrupt_temporal" => {
            let rid = as_str(request, "resource");
            let rk = key(source, generation, rid);
            let mut row = read_value(db, RESOURCES, &rk);
            row["temporal"]["profile"]["freshness_basis"] = json!("tampered");
            let raw = serde_json::to_vec(&row).unwrap();
            let tx = db.begin_write().unwrap();
            {
                let mut resources = tx.open_table(RESOURCES).unwrap();
                resources.insert(rk.as_str(), raw.as_slice()).unwrap();
            }
            tx.commit().unwrap();
            json!(true)
        }
        "corrupt_mapping" => {
            let rid = as_str(request, "resource");
            let rk = key(source, generation, rid);
            let mut row = read_value(db, RESOURCES, &rk);
            row["mapping"]["document_id"] = json!("00000000-0000-0000-0000-000000000fff");
            let raw = serde_json::to_vec(&row).unwrap();
            let tx = db.begin_write().unwrap();
            {
                let mut resources = tx.open_table(RESOURCES).unwrap();
                resources.insert(rk.as_str(), raw.as_slice()).unwrap();
            }
            tx.commit().unwrap();
            json!(true)
        }
        _ => panic!("unknown request"),
    }
}

const WRITER_SOURCE: &str = "00000000-0000-0000-0000-000000000001";
const WRITER_GENERATION: &str = "00000000-0000-0000-0000-0000000000c8";

fn writer_variant(original: &Value) -> Value {
    let mut replacement = original.clone();
    let members = replacement["participants"].as_array_mut().unwrap();
    let products: Vec<usize> = members
        .iter()
        .enumerate()
        .filter_map(|(i, p)| (p["role"] == "product").then_some(i))
        .collect();
    assert_eq!(
        products.len(),
        2,
        "writer relation needs two product incidences"
    );
    members.swap(products[0], products[1]);
    replacement["qualifiers"]["ordered_terms"]["List"]
        .as_array_mut()
        .unwrap()
        .reverse();
    replacement
}

fn writer_loop(db: Arc<Database>, stop: Arc<AtomicBool>, commits: Arc<AtomicU64>) -> Vec<Value> {
    let relation_id = "00000000-0000-0000-0000-0000000f4240";
    let relation_key = key(WRITER_SOURCE, WRITER_GENERATION, relation_id);
    let original = read_value(&db, RELATIONS, &relation_key);
    assert!(!original.is_null(), "writer relation is absent");
    let variant = writer_variant(&original);
    let mut previous = original.clone();
    let mut use_variant = true;
    let mut updates = Vec::new();
    while !stop.load(Ordering::Acquire) {
        let next = if use_variant { &variant } else { &original };
        let start = Instant::now();
        let tx = db.begin_write().unwrap();
        {
            let mut relations = tx.open_table(RELATIONS).unwrap();
            let mut participants = tx.open_table(PARTICIPANTS).unwrap();
            let mut incidence = tx.open_table(INCIDENCE).unwrap();
            let mut membership = tx.open_table(MEMBERSHIP).unwrap();
            remove_relation(
                &mut relations,
                &mut participants,
                &mut incidence,
                &mut membership,
                WRITER_SOURCE,
                WRITER_GENERATION,
                &previous,
            );
            insert_relation(
                &mut relations,
                &mut participants,
                &mut incidence,
                &mut membership,
                WRITER_SOURCE,
                WRITER_GENERATION,
                next,
            );
        }
        tx.commit().unwrap();
        commits.fetch_add(1, Ordering::Release);
        updates.push(
            json!({"outcome":"committed","duration_ns":start.elapsed().as_nanos(),
            "relation_id":relation_id}),
        );
        previous = next.clone();
        use_variant = !use_variant;
        thread::sleep(Duration::from_millis(5));
    }
    updates
}

struct WriterState {
    stop: Arc<AtomicBool>,
    commits: Arc<AtomicU64>,
    handle: thread::JoinHandle<Vec<Value>>,
}

fn respond(output: &Mutex<io::Stdout>, id: &Value, result: Result<Value, String>) {
    let response = match result {
        Ok(value) => json!({"id":id,"ok":value}),
        Err(message) => json!({"id":id,"error":message}),
    };
    let mut out = output.lock().unwrap();
    writeln!(out, "{response}").unwrap();
    out.flush().unwrap();
}

fn process_request(
    db: &Arc<Database>,
    writer: &Arc<Mutex<Option<WriterState>>>,
    request: &Value,
) -> Value {
    match as_str(request, "cmd") {
        "server_stats" => json!({
            "max_parallel_frontier_reads": MAX_FRONTIER_READERS.load(Ordering::Acquire),
            "active_frontier_reads": ACTIVE_FRONTIER_READERS.load(Ordering::Acquire),
            "worker_threads": 8,
        }),
        "writer_start" => {
            let mut guard = writer.lock().unwrap();
            assert!(guard.is_none(), "writer is already running");
            let stop = Arc::new(AtomicBool::new(false));
            let commits = Arc::new(AtomicU64::new(0));
            let writer_db = Arc::clone(db);
            let writer_stop = Arc::clone(&stop);
            let writer_commits = Arc::clone(&commits);
            let handle = thread::spawn(move || writer_loop(writer_db, writer_stop, writer_commits));
            *guard = Some(WriterState {
                stop,
                commits,
                handle,
            });
            json!(true)
        }
        "writer_progress" => json!(
            writer
                .lock()
                .unwrap()
                .as_ref()
                .expect("writer is not running")
                .commits
                .load(Ordering::Acquire)
        ),
        "writer_stop" => {
            let state = writer
                .lock()
                .unwrap()
                .take()
                .expect("writer is not running");
            state.stop.store(true, Ordering::Release);
            json!(state.handle.join().expect("writer thread failed"))
        }
        _ => execute(db, request),
    }
}

fn serve(db_path: &str) {
    let db = Arc::new(Database::open(db_path).unwrap());
    let output = Arc::new(Mutex::new(io::stdout()));
    let writer: Arc<Mutex<Option<WriterState>>> = Arc::new(Mutex::new(None));
    let (sender, receiver) = mpsc::channel::<Value>();
    let receiver = Arc::new(Mutex::new(receiver));
    let mut workers = Vec::new();
    for _ in 0..8 {
        let db = Arc::clone(&db);
        let output = Arc::clone(&output);
        let writer = Arc::clone(&writer);
        let receiver = Arc::clone(&receiver);
        workers.push(thread::spawn(move || {
            loop {
                let request = {
                    let guard = receiver.lock().unwrap();
                    guard.recv()
                };
                let Ok(request) = request else { break };
                let id = request["id"].clone();
                let result =
                    catch_unwind(AssertUnwindSafe(|| process_request(&db, &writer, &request)))
                        .map_err(|payload| {
                            payload
                                .downcast_ref::<String>()
                                .cloned()
                                .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                                .unwrap_or_else(|| "native redb request panicked".to_owned())
                        });
                respond(&output, &id, result);
            }
        }));
    }
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        sender.send(request).unwrap();
    }
    drop(sender);
    for worker in workers {
        worker.join().unwrap();
    }
    if let Some(state) = writer.lock().unwrap().take() {
        state.stop.store(true, Ordering::Release);
        state.handle.join().unwrap();
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    match args.as_slice() {
        [_, command, fixture, db] if command == "stage" => stage(fixture, db),
        [_, command, db] if command == "serve" => serve(db),
        _ => panic!("usage: qualification_redb stage FIXTURE DB | serve DB"),
    }
}

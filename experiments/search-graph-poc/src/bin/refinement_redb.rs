//! JSON-lines native redb adapter for the P3 correctness runner.
//! `stage FIXTURE DB` creates an isolated file; `serve DB` answers scoped reads.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, BufRead, Write};

use redb::{Database, ReadableDatabase, TableDefinition};
use serde_json::{Value, json};

const RESOURCES: TableDefinition<&str, &[u8]> = TableDefinition::new("refinement_resources");
const RELATIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("refinement_relations");
const PARTICIPANTS: TableDefinition<&str, &[u8]> = TableDefinition::new("refinement_participants");
const INCIDENCE: TableDefinition<&str, &str> = TableDefinition::new("refinement_incidence");
const GENERATIONS: TableDefinition<&str, &str> = TableDefinition::new("refinement_generations");

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
    }
}

fn remove_relation(
    relations: &mut redb::Table<'_, &str, &[u8]>,
    participants: &mut redb::Table<'_, &str, &[u8]>,
    incidence: &mut redb::Table<'_, &str, &str>,
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
        for (rid, prior) in &old {
            if new.get(rid) != Some(prior) {
                remove_relation(
                    &mut relations,
                    &mut participants,
                    &mut incidence,
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
    stage_full(&db, &generations[0], None);
    stage_incremental(&db, &generations[0], &generations[1]);
    stage_full(
        &db,
        &generations[1],
        Some("00000000-0000-0000-0000-000000000066"),
    );
    stage_full(&db, &generations[2], None);
    println!(
        "{}",
        json!({"status":"ready", "transport":"redb 4.3.0 native transaction", "file_bytes":fs::metadata(db_path).unwrap().len()})
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

fn execute(db: &Database, request: &Value) -> Value {
    let command = as_str(request, "cmd");
    let source = as_str(request, "source");
    let generation = as_str(request, "generation");
    if !ready(db, source, generation) {
        return Value::Null;
    }
    match command {
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

fn serve(db_path: &str) {
    let db = Database::open(db_path).unwrap();
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        let value = execute(&db, &request);
        println!("{}", json!({"ok": value}));
        io::stdout().flush().unwrap();
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    match args.as_slice() {
        [_, command, fixture, db] if command == "stage" => stage(fixture, db),
        [_, command, db] if command == "serve" => serve(db),
        _ => panic!("usage: refinement_redb stage FIXTURE DB | serve DB"),
    }
}

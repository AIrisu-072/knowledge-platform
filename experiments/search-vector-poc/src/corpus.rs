use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use uuid::Uuid;

use crate::validate::{
    FrozenUnit, SYNTHETIC_PROFILE, sha256_hex, unit_id, validate_source_owned_unit, validate_units,
};

pub const SEED: u64 = 20260930;
pub const SCALES: [usize; 3] = [32, 256, 1024];
pub const SOURCE_ID: u128 = 1;
pub const GENERATION_ID: u128 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadState {
    Allowed,
    Denied,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct CorpusRecord {
    pub index: usize,
    pub unit: FrozenUnit,
    pub additional_units: Vec<FrozenUnit>,
    pub source_parts: Vec<SourcePart>,
    pub canonical_name: String,
    pub aliases: Vec<String>,
    pub eligible: bool,
    pub temporal_valid: bool,
    pub current_version: bool,
    pub current_read: ReadState,
}

/// Synthetic Source-owned Part facts, kept separately from a proposed Unit's fields.
#[derive(Clone, Debug)]
pub struct SourcePart {
    pub source_native_part_id: String,
    pub logical_path: String,
    pub ordinal: u32,
    pub authoritative_representation_ref: String,
    pub raw_bytes: Vec<u8>,
    pub line_start: u32,
    pub line_end: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Query {
    pub query_id: String,
    pub text: String,
    pub seed_index: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Qrel {
    pub query_id: String,
    pub resource_index: usize,
    pub grade: u8,
}

pub struct Corpus {
    pub seed: u64,
    pub source_id: Uuid,
    pub generation_id: Uuid,
    pub records: Vec<CorpusRecord>,
    pub queries: Vec<Query>,
    pub qrels: Vec<Qrel>,
}

fn parse_jsonl<T: for<'de> Deserialize<'de>>(raw: &str) -> Result<Vec<T>, String> {
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(|error| error.to_string()))
        .collect()
}

fn background_text(seed: u64, index: usize) -> String {
    let mut x = seed ^ (index as u64).wrapping_mul(0x9e3779b97f4a7c15);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    let terms = [
        "配置 台帳",
        "運用 記録",
        "保管 案内",
        "変更 履歴",
        "分類 参照",
    ];
    format!(
        "合成項目 {index:04} {} {:04x}",
        terms[x as usize % terms.len()],
        x & 0xffff
    )
}

fn source_part(
    resource_id: Uuid,
    part_id: Uuid,
    logical_path: &str,
    ordinal: u32,
    text: &str,
) -> SourcePart {
    SourcePart {
        source_native_part_id: part_id.to_string(),
        logical_path: logical_path.into(),
        ordinal,
        authoritative_representation_ref: format!(
            "synthetic-representation:{resource_id}:{logical_path}"
        ),
        raw_bytes: text.as_bytes().to_vec(),
        line_start: 0,
        line_end: text.lines().count() as u32,
    }
}

fn unit_from_source_part(
    source_id: Uuid,
    resource_id: Uuid,
    generation_id: Uuid,
    version: &str,
    part: &SourcePart,
) -> Result<FrozenUnit, String> {
    let text = String::from_utf8(part.raw_bytes.clone()).map_err(|e| e.to_string())?;
    let mut unit = FrozenUnit {
        source_id,
        resource_id,
        generation_id,
        source_snapshot: "synthetic-snapshot-v1".into(),
        source_native_version: version.into(),
        source_native_part_id: part.source_native_part_id.clone(),
        logical_path: part.logical_path.clone(),
        part_ordinal: part.ordinal,
        profile: SYNTHETIC_PROFILE.into(),
        parser_build_id: "synthetic-trusted-projection-v1".into(),
        authoritative_representation_ref: part.authoritative_representation_ref.clone(),
        raw_sha256: sha256_hex(&part.raw_bytes),
        raw_size_bytes: part.raw_bytes.len() as u64,
        raw_bytes: part.raw_bytes.clone(),
        text_sha256: sha256_hex(text.as_bytes()),
        text,
        line_start: part.line_start,
        line_end: part.line_end,
        unit_ordinal: 0,
        unit_id: String::new(),
    };
    unit.unit_id = unit_id(&unit)?;
    Ok(unit)
}

impl Corpus {
    pub fn synthetic(scale: usize, seed: u64) -> Result<Self, String> {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../corpus_manifest.json"))
                .map_err(|e| e.to_string())?;
        if seed != SEED || !SCALES.contains(&scale) || manifest["seed"] != seed {
            return Err("unsupported fixture seed or scale".into());
        }
        let queries: Vec<Query> = parse_jsonl(include_str!("../queries.jsonl"))?;
        let qrels: Vec<Qrel> = parse_jsonl(include_str!("../qrels.jsonl"))?;
        let source_id = Uuid::from_u128(SOURCE_ID);
        let generation_id = Uuid::from_u128(GENERATION_ID);
        let mut records = Vec::with_capacity(scale);
        for index in 0..scale {
            let resource_id = Uuid::from_u128(100 + index as u128);
            let group = index / 10;
            let slot = index % 10;
            let (text, canonical_name, aliases) = if group < 3 {
                let term = &queries[group].text;
                match slot {
                    0 => (format!("{term} 本文 合成記録"), term.clone(), vec![]),
                    1 => (
                        format!("{term} 参照 合成記録"),
                        format!("文書 {index}"),
                        vec![term.clone()],
                    ),
                    2 | 3 => (
                        format!("関連資料 {index} 固有論点"),
                        format!("関連資料 {index}"),
                        vec![],
                    ),
                    4 => (
                        format!("起点 {index} 概念"),
                        format!("起点 {index}"),
                        vec![],
                    ),
                    5 => (
                        format!("関係 文脈 {index}"),
                        format!("文脈 {index}"),
                        vec![],
                    ),
                    6 | 8 | 9 => (format!("{term} 制限資料 {index}"), term.clone(), vec![]),
                    7 => (
                        format!("周辺情報 {index}"),
                        format!("周辺情報 {index}"),
                        vec![],
                    ),
                    _ => unreachable!(),
                }
            } else {
                let text = background_text(seed, index);
                (text.clone(), text, vec![])
            };
            let version = Uuid::from_u128(1_000_000 + index as u128).to_string();
            let primary = source_part(
                resource_id,
                Uuid::from_u128(2_000_000 + index as u128),
                "primary",
                0,
                &text,
            );
            let mut source_parts = vec![primary];
            if index == 0 {
                source_parts.push(source_part(
                    resource_id,
                    Uuid::from_u128(3_000_000 + index as u128),
                    "attachment",
                    1,
                    "合成前置き\n監査 証跡 添付専用語 合成記録",
                ));
            }
            let unit = unit_from_source_part(
                source_id,
                resource_id,
                generation_id,
                &version,
                &source_parts[0],
            )?;
            let additional_units = source_parts
                .iter()
                .skip(1)
                .map(|part| {
                    unit_from_source_part(source_id, resource_id, generation_id, &version, part)
                })
                .collect::<Result<Vec<_>, _>>()?;
            records.push(CorpusRecord {
                index,
                unit,
                additional_units,
                source_parts,
                canonical_name,
                aliases: if group < 3 && matches!(slot, 6 | 8) {
                    let mut aliases = aliases;
                    aliases.push("秘匿固有語".into());
                    aliases
                } else if index == 31 {
                    let mut aliases = aliases;
                    aliases.push("無関連可視語".into());
                    aliases
                } else {
                    aliases
                },
                eligible: group >= 3 || !matches!(slot, 4 | 5 | 7),
                temporal_valid: group >= 3 || slot != 9,
                current_version: true,
                current_read: if group < 3 && slot == 6 {
                    ReadState::Denied
                } else if group < 3 && slot == 8 {
                    ReadState::Unknown
                } else {
                    ReadState::Allowed
                },
            });
        }
        let corpus = Self {
            seed,
            source_id,
            generation_id,
            records,
            queries,
            qrels,
        };
        corpus.validate()?;
        Ok(corpus)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_units(
            &self
                .records
                .iter()
                .flat_map(|r| std::iter::once(&r.unit).chain(r.additional_units.iter()))
                .cloned()
                .collect::<Vec<_>>(),
            self.generation_id,
        )?;
        if self.records.len() < 32 || self.records.iter().enumerate().any(|(i, r)| r.index != i) {
            return Err("resource enumeration mismatch".into());
        }
        for record in &self.records {
            let unit = &record.unit;
            if unit.source_id != self.source_id
                || unit.resource_id != Uuid::from_u128(100 + record.index as u128)
                || unit.source_native_version
                    != Uuid::from_u128(1_000_000 + record.index as u128).to_string()
                || unit.source_native_part_id
                    != Uuid::from_u128(2_000_000 + record.index as u128).to_string()
                || unit.logical_path != "primary"
                || unit.part_ordinal != 0
            {
                return Err("Source-owned Version/Part/Unit binding mismatch".into());
            }
            if record.source_parts.len() != 1 + record.additional_units.len()
                || record.source_parts.len() != if record.index == 0 { 2 } else { 1 }
            {
                return Err("Source Part enumeration mismatch".into());
            }
            let mut part_ids = BTreeSet::new();
            for (part, proposed) in record
                .source_parts
                .iter()
                .zip(std::iter::once(&record.unit).chain(record.additional_units.iter()))
            {
                if !part_ids.insert(&part.source_native_part_id) {
                    return Err("duplicate Source Part".into());
                }
                validate_source_owned_unit(
                    self.source_id,
                    Uuid::from_u128(100 + record.index as u128),
                    self.generation_id,
                    &Uuid::from_u128(1_000_000 + record.index as u128).to_string(),
                    part,
                    proposed,
                )?;
            }
        }
        let query_ids: BTreeSet<_> = self.queries.iter().map(|q| q.query_id.as_str()).collect();
        if query_ids.len() != self.queries.len()
            || !["q0", "q1", "q2", "qpart", "qnone", "qaccess", "qfalse"]
                .iter()
                .all(|id| query_ids.contains(id))
            || query_ids.len() != 7
        {
            return Err("query IDs invalid".into());
        }
        let mut qrels_by_query: BTreeMap<&str, Vec<&Qrel>> = BTreeMap::new();
        let mut pairs = BTreeSet::new();
        for qrel in &self.qrels {
            if !query_ids.contains(qrel.query_id.as_str())
                || qrel.grade > 3
                || qrel.resource_index >= self.records.len()
                || !pairs.insert((qrel.query_id.as_str(), qrel.resource_index))
            {
                return Err("invalid/duplicate qrel".into());
            }
            if qrel.grade > 0 {
                let record = &self.records[qrel.resource_index];
                if !record.eligible
                    || !record.temporal_valid
                    || !record.current_version
                    || record.current_read != ReadState::Allowed
                {
                    return Err("positive qrel is not an eligible parent".into());
                }
            }
            qrels_by_query.entry(&qrel.query_id).or_default().push(qrel);
        }
        for query in self
            .queries
            .iter()
            .filter(|q| ["q0", "q1", "q2"].contains(&q.query_id.as_str()))
        {
            let positives = qrels_by_query
                .get(query.query_id.as_str())
                .ok_or("missing query qrels")?;
            if positives.iter().filter(|q| q.grade > 0).count() < 3
                || !positives
                    .iter()
                    .any(|q| q.grade > 0 && q.resource_index % 10 == 2)
                || !positives
                    .iter()
                    .any(|q| q.grade > 0 && q.resource_index % 10 == 3)
            {
                return Err("multiple positive graph-only parents required".into());
            }
        }
        if qrels_by_query.get("qpart").is_none_or(|qrels| {
            qrels.len() != 1 || qrels[0].resource_index != 0 || qrels[0].grade == 0
        }) {
            return Err("attachment Part must have one parent qrel".into());
        }
        if ["qnone", "qaccess", "qfalse"]
            .iter()
            .any(|id| qrels_by_query.contains_key(id))
        {
            return Err("negative query must have zero positives".into());
        }
        Ok(())
    }
}

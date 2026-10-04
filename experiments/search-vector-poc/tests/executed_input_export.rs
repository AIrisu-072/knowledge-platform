use std::process::Command;

use search_vector_poc::corpus::{Corpus, SEED};
use search_vector_poc::run::export_synthetic_input;
use serde_json::Value;

#[test]
fn exported_input_is_the_corpus_and_planner_used_by_the_baseline() {
    let corpus = Corpus::synthetic(32, SEED).expect("frozen synthetic corpus");
    let exported = export_synthetic_input(&corpus, 20).expect("executed input export");
    assert_eq!(exported["schema"], "p2-synthetic-executed-input-v1");
    assert_eq!(exported["scale"], 32);
    assert_eq!(exported["seed"], SEED);
    assert_eq!(exported["actor"], "synthetic-principal");

    let rows = exported["artifacts"]["corpus_text_sha256"]["rows"]
        .as_array()
        .expect("rows");
    assert_eq!(
        rows.len(),
        33,
        "resource zero has two Units in distinct Parts"
    );
    assert_eq!(rows[0]["text"], corpus.records[0].unit.text);
    assert_eq!(rows[1]["text"], corpus.records[0].additional_units[0].text);
    assert_eq!(
        rows[1]["part_id"],
        corpus.records[0].additional_units[0].source_native_part_id
    );
    assert_eq!(
        rows[1]["representation_ref"],
        corpus.records[0].additional_units[0].authoritative_representation_ref
    );
    assert_eq!(
        exported["artifacts"]["current_read_sha256"]["denied"]
            .as_array()
            .expect("denied parents")
            .len(),
        3
    );
    assert_eq!(
        exported["artifacts"]["current_read_sha256"]["unknown"]
            .as_array()
            .expect("unknown parents")
            .len(),
        3
    );
    let plans = exported["artifacts"]["planner_output"]["baseline_plans"]
        .as_array()
        .expect("actual planner output");
    assert_eq!(plans.len(), 14, "seven queries for each L/LG arm");
    assert!(
        plans
            .iter()
            .any(|plan| plan["arm"] == "L" && plan["query_id"] == "q0")
    );
    assert!(
        plans
            .iter()
            .any(|plan| plan["arm"] == "LG" && plan["query_id"] == "q0")
    );

    let output = Command::new(env!("CARGO_BIN_EXE_search-vector-poc"))
        .args(["--export-synthetic-input", "32", "20"])
        .output()
        .expect("baseline export executable");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let from_executable: Value = serde_json::from_slice(&output.stdout).expect("JSON export");
    assert_eq!(
        from_executable, exported,
        "the CLI must export the same in-process input"
    );
}

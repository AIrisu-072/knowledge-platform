use lindera::dictionary::load_dictionary;
use lindera::mode::Mode;
use lindera::segmenter::Segmenter;
use search_discovery_poc::lexical::{
    AnalyzerKind, LexicalCase, LexicalResource, evaluate_lexical, segment_lindera_text,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Golden {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    #[serde(rename = "tokenCases")]
    token_cases: Vec<TokenCase>,
    evaluations: Vec<Value>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenCase {
    id: String,
    text: String,
    tokens: Value,
}

fn fixtures() -> (Vec<LexicalResource>, Vec<LexicalCase>) {
    (
        serde_json::from_str(include_str!("../fixtures/lexical/resources.json")).unwrap(),
        serde_json::from_str(include_str!("../fixtures/lexical/queries.json")).unwrap(),
    )
}
fn golden() -> Golden {
    serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/tokenizer-6.2.0-golden.json"
        ))
        .expect("old no-filter Tokenizer golden must be generated from verified baseline"),
    )
    .unwrap()
}
fn all_inputs() -> Vec<(String, String)> {
    let (resources, cases) = fixtures();
    let mut inputs = Vec::new();
    for resource in resources {
        inputs.push((format!("resource:{}:title", resource.id), resource.title));
        inputs.push((
            format!("resource:{}:summary", resource.id),
            resource.summary,
        ));
        for (index, alias) in resource.aliases.into_iter().enumerate() {
            inputs.push((format!("resource:{}:alias:{index}", resource.id), alias));
        }
    }
    inputs.extend(
        cases
            .into_iter()
            .map(|case| (format!("query:{}", case.id), case.query)),
    );
    for (index, text) in [
        "",
        "ABC123",
        "! ? / ()",
        "令和8年 AI検索 2026-10-14。",
        "融資\n相談\t窓口",
    ]
    .into_iter()
    .enumerate()
    {
        inputs.push((format!("edge:{index}"), text.to_owned()));
    }
    inputs
}

#[test]
fn production_segmentation_matches_old_no_filter_tokenizer_for_all_fixture_and_edge_inputs() {
    let expected = golden();
    assert_eq!(expected.schema_version, 1);
    let inputs = all_inputs();
    assert_eq!(
        expected.token_cases.len(),
        inputs.len(),
        "golden cannot omit fixture inputs"
    );
    let segmenter = Segmenter::new(
        Mode::Normal,
        load_dictionary("embedded://ipadic").unwrap(),
        None,
    );
    for (input, case) in inputs.iter().zip(expected.token_cases) {
        assert_eq!(
            (&case.id, &case.text),
            (&input.0, &input.1),
            "golden fixture order/source drift"
        );
        let actual:Vec<Value>=segment_lindera_text(&segmenter,&case.text).unwrap().iter().map(|token|json!({
            "surface":token.surface.as_ref(),"byte_start":token.byte_start,"byte_end":token.byte_end,
            "position":token.position,"position_length":token.position_length,
        })).collect();
        assert_eq!(
            json!(actual),
            case.tokens,
            "token order/surface/offset/position changed for {}",
            case.id
        );
    }
}

#[test]
fn production_search_preserves_every_case_quality_missing_ids_and_index_size() {
    let expected = golden();
    assert_eq!(expected.evaluations.len(), 2);
    let (resources, cases) = fixtures();
    for (kind, baseline) in [AnalyzerKind::TantivyDefault, AnalyzerKind::LinderaIpadic]
        .into_iter()
        .zip(expected.evaluations)
    {
        let result = evaluate_lexical(kind, &resources, &cases).unwrap();
        assert_eq!(result.cases.len(), cases.len());
        let actual = json!({"analyzer_id":result.measurement.analyzer_id,"cases":result.cases,
            "recall_at_10":result.measurement.recall_at_10,"mrr":result.measurement.mrr,
            "ndcg_at_10":result.measurement.ndcg_at_10,"index_bytes":result.measurement.index_bytes,
            "quality_bits": {"recall_at_10":result.measurement.recall_at_10.to_bits(),
                "mrr":result.measurement.mrr.to_bits(),"ndcg_at_10":result.measurement.ndcg_at_10.to_bits()}});
        // Require bit-exact floating measurements independently of decimal JSON parsing.
        assert_eq!(
            actual["quality_bits"], baseline["quality_bits"],
            "quality float bits changed"
        );
        // Both report projections cross the same JSON boundary; decimal parser rounding
        // cannot hide a quality difference because the bit guard above is exact.
        let actual: Value = serde_json::from_str(&serde_json::to_string(&actual).unwrap()).unwrap();
        assert_eq!(
            actual, baseline,
            "all retrieved/qualified IDs (including misses), quality, and index size must remain identical"
        );
    }
}

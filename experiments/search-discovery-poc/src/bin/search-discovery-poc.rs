use search_discovery_poc::lexical::{AnalyzerKind, LexicalCase, LexicalResource, evaluate_lexical};
use search_discovery_poc::report::QualificationReport;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let report = QualificationReport::harness();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["verify"] => {
            report.verify_harness()?;
            println!("harness verified");
        }
        ["report", "--format", "json"] => {
            println!("{}", serde_json::to_string(&report)?);
        }
        ["measure-lexical", "--format", "json"] => {
            let resources: Vec<LexicalResource> =
                serde_json::from_str(include_str!("../../fixtures/lexical/resources.json"))?;
            let cases: Vec<LexicalCase> =
                serde_json::from_str(include_str!("../../fixtures/lexical/queries.json"))?;
            let mut evaluations = Vec::new();
            for _ in 0..5 {
                for analyzer in [AnalyzerKind::TantivyDefault, AnalyzerKind::LinderaIpadic] {
                    evaluations.push(evaluate_lexical(analyzer, &resources, &cases)?);
                }
            }
            println!("{}", serde_json::to_string(&evaluations)?);
        }
        _ => {
            return Err(
                "expected 'verify', 'report --format json', or 'measure-lexical --format json'"
                    .into(),
            );
        }
    }
    Ok(())
}

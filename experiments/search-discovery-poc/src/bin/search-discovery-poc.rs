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
        _ => return Err("expected 'verify' or 'report --format json'".into()),
    }
    Ok(())
}

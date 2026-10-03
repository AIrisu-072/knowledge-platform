use std::process::Command;
use std::time::Duration;

use crate::run::{BuildTiming, QueryRun};

#[derive(Clone, Debug)]
pub struct Percentiles {
    pub samples: usize,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
}

fn percentile(values: &[f64], p: f64) -> f64 {
    let index = ((values.len() as f64 * p).ceil() as usize).saturating_sub(1);
    values[index.min(values.len() - 1)]
}

pub fn percentiles(samples: &[Duration]) -> Option<Percentiles> {
    if samples.is_empty() {
        return None;
    }
    let mut values = samples
        .iter()
        .map(|value| value.as_secs_f64() * 1000.0)
        .collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    Some(Percentiles {
        samples: values.len(),
        p50_ms: percentile(&values, 0.50),
        p95_ms: percentile(&values, 0.95),
        p99_ms: percentile(&values, 0.99),
    })
}

/// Current process resident set from the host `ps` command, in KiB.
pub fn current_rss_kib() -> Result<u64, String> {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("ps RSS sample failed".into());
    }
    String::from_utf8(output.stdout)
        .map_err(|e| e.to_string())?
        .trim()
        .parse()
        .map_err(|e: std::num::ParseIntError| e.to_string())
}

pub struct Measurements {
    pub build_lexical_ms: f64,
    pub build_graph_ms: f64,
    pub update_lexical_ms: f64,
    pub update_graph_ms: f64,
    pub query: Percentiles,
    pub cold_query_ms: f64,
    pub warm_query: Percentiles,
    pub fusion: Percentiles,
    pub rss_kib: u64,
    pub index_disk_bytes: u64,
}

pub fn measure_run(
    runs: &[QueryRun],
    build: &BuildTiming,
    update: &BuildTiming,
    rss_kib: u64,
) -> Result<Measurements, String> {
    Ok(Measurements {
        build_lexical_ms: build.lexical.as_secs_f64() * 1000.0,
        build_graph_ms: build.graph.as_secs_f64() * 1000.0,
        update_lexical_ms: update.lexical.as_secs_f64() * 1000.0,
        update_graph_ms: update.graph.as_secs_f64() * 1000.0,
        query: percentiles(&runs.iter().map(|r| r.query_time).collect::<Vec<_>>())
            .ok_or("no query samples")?,
        cold_query_ms: runs
            .first()
            .ok_or("no cold query sample")?
            .query_time
            .as_secs_f64()
            * 1000.0,
        warm_query: percentiles(
            &runs
                .iter()
                .skip(1)
                .map(|r| r.query_time)
                .collect::<Vec<_>>(),
        )
        .ok_or("no warm query samples")?,
        fusion: percentiles(&runs.iter().map(|r| r.fusion_time).collect::<Vec<_>>())
            .ok_or("no fusion samples")?,
        rss_kib,
        // TantivyLexicalIndex and MemoryGraphRetriever both use in-process RAM stores.
        index_disk_bytes: 0,
    })
}

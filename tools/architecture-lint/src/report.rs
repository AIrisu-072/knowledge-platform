use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn status(&self) -> &'static str {
        if self.findings.is_empty() {
            "pass"
        } else {
            "fail"
        }
    }
}

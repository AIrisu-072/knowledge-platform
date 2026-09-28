use search_discovery_poc::hypergraph::{Participant, Relation};

#[derive(Debug, Clone, Copy)]
pub enum DegreeProfile {
    Sparse,
    Moderate,
    High,
}

pub fn synthetic_graph(profile: DegreeProfile, relation_count: usize) -> Vec<Relation> {
    (0..relation_count)
        .map(|number| {
            let borrower = match profile {
                DegreeProfile::Sparse => format!("company-{number}"),
                DegreeProfile::Moderate => format!("company-{}", number / 8),
                DegreeProfile::High => "company-root".into(),
            };
            Relation {
                id: format!("generated-{number:05}"),
                namespace: "discovery".into(),
                relation_type: "loan".into(),
                participants: vec![
                    Participant::new("borrower", borrower),
                    Participant::new("product", format!("product-{number}")),
                    Participant::new("collateral", format!("collateral-{number}")),
                    Participant::new("branch", format!("branch-{}", number % 4)),
                ],
            }
        })
        .collect()
}

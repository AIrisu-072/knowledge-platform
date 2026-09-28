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
            let parent = match profile {
                DegreeProfile::Sparse => format!("parent-{number}"),
                DegreeProfile::Moderate => format!("parent-{}", number / 8),
                DegreeProfile::High => "concept-root".into(),
            };
            Relation {
                id: format!("generated-{number:05}"),
                namespace: "semantic".into(),
                relation_type: "is_a".into(),
                participants: vec![
                    Participant::new("parent", parent),
                    Participant::new("child", format!("concept-{number}")),
                ],
            }
        })
        .collect()
}

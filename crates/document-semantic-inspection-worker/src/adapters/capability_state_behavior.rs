use super::SemanticAdapterOutput;
use crate::WorkerFailureCode;
use document_semantic_inspection_core::CapabilityState;

fn output_with_capabilities(capability_ids: &[&str]) -> SemanticAdapterOutput {
    SemanticAdapterOutput::from_projection(b"{}", capability_ids, "test", &[])
}

#[test]
fn unknown_capability_id_returns_invalid_worker_result() {
    let failure = output_with_capabilities(&["reader_content"])
        .with_capability_state("unknown_capability", CapabilityState::Present)
        .expect_err("unknown capability IDs must fail closed");

    assert_eq!(failure.code(), WorkerFailureCode::InvalidWorkerResult);
}

#[test]
fn present_known_capability_keeps_the_semantic_fingerprint() {
    let original = output_with_capabilities(&["reader_content"]);
    let fingerprint = original.semantic_fingerprint();
    let output = original
        .with_capability_state("reader_content", CapabilityState::Present)
        .expect("known capability should be updated");
    let capability = output
        .semantic_capabilities()
        .first()
        .expect("test output has one capability");

    assert_eq!(output.semantic_fingerprint(), fingerprint);
    assert_eq!(capability.presence, CapabilityState::Present);
    assert_eq!(capability.equivalence_fingerprint, Some(fingerprint));
}

#[test]
fn non_present_known_capabilities_have_no_equivalence_fingerprint() {
    for state in [
        CapabilityState::Absent,
        CapabilityState::NotRepresentable,
        CapabilityState::NotVerifiable,
    ] {
        let original = output_with_capabilities(&["footnotes"]);
        let fingerprint = original.semantic_fingerprint();
        let output = original
            .with_capability_state("footnotes", state)
            .expect("known capability should be updated");
        let capability = output
            .semantic_capabilities()
            .first()
            .expect("test output has one capability");

        assert_eq!(output.semantic_fingerprint(), fingerprint);
        assert_eq!(capability.presence, state);
        assert_eq!(capability.equivalence_fingerprint, None);
    }
}

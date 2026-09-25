use document_semantic_inspection_core::{
    ExtractorProvenance, NativeDependencyIdentity, ParserLibraryIdentity,
};

pub fn worker_build_id() -> &'static str {
    option_env!("DSI_WORKER_BUILD_ID")
        .unwrap_or(concat!(env!("CARGO_PKG_NAME"), "@", env!("CARGO_PKG_VERSION")))
}

pub fn extractor_provenance(
    adapter_id: impl Into<String>,
    adapter_version: impl Into<String>,
    parser_libraries: Vec<ParserLibraryIdentity>,
    native_dependency_identity: Vec<NativeDependencyIdentity>,
) -> ExtractorProvenance {
    ExtractorProvenance {
        worker_build_id: worker_build_id().to_owned(),
        adapter_id: adapter_id.into(),
        adapter_version: adapter_version.into(),
        parser_libraries,
        native_dependency_identity,
    }
}

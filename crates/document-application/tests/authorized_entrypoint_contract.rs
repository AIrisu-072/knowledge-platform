use document_application::{
    AuthorizationScope, AuthorizedDocumentService, CancelScheduleCommand, Clock,
    CreateDocumentCommand, CreateVersionCommand, DocumentPublishRepository, DocumentRepository,
    EndDocumentPublicationCommand, FileStorage, IdGenerator, PublicationEndRepository,
    PublicationScheduleRepository, PublishDocumentCommand, RebaseWorkingVersionCommand,
    SchedulePublishCommand, SemanticInspectionExecutor, SemanticInspectionRepository,
    UpdateWorkingVersionCommand, VerifiedActorContext, VersioningRepository,
    WithdrawVersionCommand,
};

// Type-check every mutating transport-facing method with an explicit verified
// context. There is no context-free overload on this service.
#[allow(dead_code, clippy::too_many_arguments)]
async fn mutation_surface_requires_context<I, C, F, E, R>(
    service: &AuthorizedDocumentService<I, C, F, E, R>,
    ctx: &VerifiedActorContext,
    create_document: CreateDocumentCommand,
    create_version: CreateVersionCommand,
    update_working: UpdateWorkingVersionCommand,
    rebase_working: RebaseWorkingVersionCommand,
    publish: PublishDocumentCommand,
    withdraw: WithdrawVersionCommand,
    schedule: SchedulePublishCommand,
    cancel: CancelScheduleCommand,
    end_publication: EndDocumentPublicationCommand,
    prepared: document_application::PreparedManifest,
) where
    I: IdGenerator,
    C: Clock,
    F: FileStorage,
    E: SemanticInspectionExecutor,
    R: AuthorizationScope
        + DocumentRepository
        + DocumentPublishRepository
        + VersioningRepository
        + SemanticInspectionRepository
        + PublicationScheduleRepository
        + PublicationEndRepository,
{
    let _ = service.create_document(ctx, create_document).await;
    let _ = service
        .create_version(ctx, create_version, prepared.clone())
        .await;
    let _ = service
        .update_working_version(ctx, update_working, prepared)
        .await;
    let _ = service.rebase_working_version(ctx, rebase_working).await;
    let _ = service.publish_document(ctx, publish).await;
    let _ = service.withdraw_version(ctx, withdraw).await;
    let _ = service.schedule_publish(ctx, schedule).await;
    let _ = service.cancel_schedule(ctx, cancel).await;
    let _ = service.end_document_publication(ctx, end_publication).await;
}

#[test]
fn scoped_entrypoint_has_no_optional_or_default_identity() {
    let source = include_str!("../src/authorized_document.rs");
    assert!(!source.contains("Option<&VerifiedActorContext>"));
    assert!(!source.contains("unwrap_or_default"));
    assert!(source.contains("fn scoped_repository(&self, ctx: &VerifiedActorContext)"));
}

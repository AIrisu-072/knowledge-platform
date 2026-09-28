use std::sync::Arc;

use document_domain::{DocumentId, PrincipalRef};

use crate::{
    ApplicationError, AuthoritativeDocument, CancelScheduleCommand, CancelScheduleResult, Clock,
    CreateDocumentCommand, CreateDocumentResult, CreateVersionCommand,
    DocumentPublicationEndService, DocumentPublishRepository, DocumentRepository, DocumentService,
    DocumentVersionService, EndDocumentPublicationCommand, EndDocumentPublicationResult,
    FileStorage, IdGenerator, PreparedManifest, PublicationEndRepository,
    PublicationScheduleRepository, PublishDocumentCommand, PublishDocumentResult,
    RebaseWorkingVersionCommand, SchedulePublishCommand, SchedulePublishResult,
    SemanticInspectionExecutor, SemanticInspectionRepository, UpdateWorkingVersionCommand,
    VerifiedActorContext, VersionOperationResult, VersioningRepository, WithdrawVersionCommand,
    WithdrawVersionResult,
};

impl<I, C, F, E, R> AuthorizedDocumentService<I, C, F, E, R>
where
    F: FileStorage,
    R: crate::VersionFileAccessRepository,
{
    pub async fn open_version_file(
        &self,
        ctx: &VerifiedActorContext,
        request: crate::VersionFileRequest,
    ) -> Result<crate::OpenedVersionFile, ApplicationError> {
        crate::VersionFileAccessService::new(self.repository.clone(), self.storage.clone())
            .open_version_file(ctx, request)
            .await
    }
}

impl<I, C, F, E, R> AuthorizedDocumentService<I, C, F, E, R>
where
    R: crate::DocumentHistoryRepository,
{
    pub async fn list_document_versions(
        &self,
        ctx: &VerifiedActorContext,
        query: crate::VersionPageQuery,
    ) -> Result<crate::Page<crate::VersionSummary>, ApplicationError> {
        crate::DocumentHistoryService::new(self.repository.clone())
            .list_document_versions(ctx, query)
            .await
    }

    pub async fn list_document_history(
        &self,
        ctx: &VerifiedActorContext,
        query: crate::HistoryPageQuery,
    ) -> Result<crate::Page<crate::DocumentHistoryEntry>, ApplicationError> {
        crate::DocumentHistoryService::new(self.repository.clone())
            .list_document_history(ctx, query)
            .await
    }

    pub async fn get_document_version(
        &self,
        ctx: &VerifiedActorContext,
        request: crate::VersionRequest,
    ) -> Result<crate::VersionDetail, ApplicationError> {
        crate::DocumentHistoryService::new(self.repository.clone())
            .get_document_version(ctx, request)
            .await
    }

    pub async fn list_version_files(
        &self,
        ctx: &VerifiedActorContext,
        request: crate::VersionRequest,
    ) -> Result<Vec<crate::VersionFileSummary>, ApplicationError> {
        crate::DocumentHistoryService::new(self.repository.clone())
            .list_version_files(ctx, request)
            .await
    }
}

/// Binds one verified actor to a repository instance without mutating a shared
/// repository. The scoped adapter must reauthorize inside every commit transaction.
pub trait AuthorizationScope: Send + Sync + Sized {
    fn with_verified_actor(&self, ctx: VerifiedActorContext) -> Self;
}

pub struct AuthorizedDocumentService<I, C, F, E, R> {
    ids: Arc<I>,
    clock: Arc<C>,
    storage: Arc<F>,
    executor: Arc<E>,
    repository: Arc<R>,
}

impl<I, C, F, E, R> AuthorizedDocumentService<I, C, F, E, R>
where
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
    pub fn new(
        ids: Arc<I>,
        clock: Arc<C>,
        storage: Arc<F>,
        executor: Arc<E>,
        repository: Arc<R>,
    ) -> Self {
        Self {
            ids,
            clock,
            storage,
            executor,
            repository,
        }
    }

    fn scoped_repository(&self, ctx: &VerifiedActorContext) -> Arc<R> {
        Arc::new(self.repository.with_verified_actor(ctx.clone()))
    }

    fn verify_actor(
        ctx: &VerifiedActorContext,
        command_actor: &PrincipalRef,
    ) -> Result<(), ApplicationError> {
        ctx.ensure_current()?;
        if ctx.principal() != command_actor {
            return Err(ApplicationError::Forbidden);
        }
        Ok(())
    }

    fn document_service(&self, ctx: &VerifiedActorContext) -> DocumentService<I, C, F, R> {
        DocumentService::new(
            self.ids.clone(),
            self.clock.clone(),
            self.storage.clone(),
            self.scoped_repository(ctx),
        )
    }

    fn version_service(&self, ctx: &VerifiedActorContext) -> DocumentVersionService<I, C, F, E, R> {
        DocumentVersionService::new(
            self.ids.clone(),
            self.clock.clone(),
            self.storage.clone(),
            self.executor.clone(),
            self.scoped_repository(ctx),
        )
    }

    pub async fn create_document(
        &self,
        ctx: &VerifiedActorContext,
        command: CreateDocumentCommand,
    ) -> Result<CreateDocumentResult, ApplicationError> {
        Self::verify_actor(ctx, &command.principal)?;
        self.document_service(ctx).create_document(command).await
    }

    pub async fn get_current_published_document(
        &self,
        ctx: &VerifiedActorContext,
        document_id: DocumentId,
    ) -> Result<AuthoritativeDocument, ApplicationError> {
        ctx.ensure_current()?;
        self.document_service(ctx)
            .get_current_published_document(document_id)
            .await
    }

    pub async fn get_authoring_document(
        &self,
        ctx: &VerifiedActorContext,
        document_id: DocumentId,
    ) -> Result<AuthoritativeDocument, ApplicationError> {
        ctx.ensure_current()?;
        self.document_service(ctx).get_document(document_id).await
    }

    pub async fn create_version(
        &self,
        ctx: &VerifiedActorContext,
        command: CreateVersionCommand,
        prepared: PreparedManifest,
    ) -> Result<VersionOperationResult, ApplicationError> {
        Self::verify_actor(ctx, command.actor())?;
        self.version_service(ctx)
            .create_version(command, prepared)
            .await
    }

    pub async fn update_working_version(
        &self,
        ctx: &VerifiedActorContext,
        command: UpdateWorkingVersionCommand,
        prepared: PreparedManifest,
    ) -> Result<VersionOperationResult, ApplicationError> {
        Self::verify_actor(ctx, command.actor())?;
        self.version_service(ctx)
            .update_working(command, prepared)
            .await
    }

    pub async fn rebase_working_version(
        &self,
        ctx: &VerifiedActorContext,
        command: RebaseWorkingVersionCommand,
    ) -> Result<VersionOperationResult, ApplicationError> {
        Self::verify_actor(ctx, command.actor())?;
        self.version_service(ctx).rebase_working(command).await
    }

    pub async fn publish_document(
        &self,
        ctx: &VerifiedActorContext,
        command: PublishDocumentCommand,
    ) -> Result<PublishDocumentResult, ApplicationError> {
        Self::verify_actor(ctx, command.principal())?;
        self.version_service(ctx).publish_document(command).await
    }

    pub async fn withdraw_version(
        &self,
        ctx: &VerifiedActorContext,
        command: WithdrawVersionCommand,
    ) -> Result<WithdrawVersionResult, ApplicationError> {
        Self::verify_actor(ctx, command.actor())?;
        self.version_service(ctx).withdraw_version(command).await
    }

    pub async fn schedule_publish(
        &self,
        ctx: &VerifiedActorContext,
        command: SchedulePublishCommand,
    ) -> Result<SchedulePublishResult, ApplicationError> {
        Self::verify_actor(ctx, command.actor())?;
        self.version_service(ctx).schedule_publish(command).await
    }

    pub async fn cancel_schedule(
        &self,
        ctx: &VerifiedActorContext,
        command: CancelScheduleCommand,
    ) -> Result<CancelScheduleResult, ApplicationError> {
        Self::verify_actor(ctx, command.actor())?;
        self.version_service(ctx).cancel_schedule(command).await
    }

    pub async fn end_document_publication(
        &self,
        ctx: &VerifiedActorContext,
        command: EndDocumentPublicationCommand,
    ) -> Result<EndDocumentPublicationResult, ApplicationError> {
        Self::verify_actor(ctx, command.actor())?;
        DocumentPublicationEndService::new(
            self.ids.clone(),
            self.clock.clone(),
            self.scoped_repository(ctx),
        )
        .end_document_publication(command)
        .await
    }
}

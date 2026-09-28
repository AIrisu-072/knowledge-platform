use document_application::{AuthorizationScope, VerifiedActorContext};

use crate::PostgresDocumentRepository;

impl AuthorizationScope for PostgresDocumentRepository {
    fn with_verified_actor(&self, ctx: VerifiedActorContext) -> Self {
        PostgresDocumentRepository::with_verified_actor(self, ctx)
    }
}

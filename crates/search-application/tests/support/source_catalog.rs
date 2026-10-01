//! Test-owned complete host inventory for the legacy Remote catalog cases.
//! The production catalog receives only captured namespace snapshots.

use std::ops::Deref;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use search_application::SearchError;
use search_application::remote_registration::RemoteSourceRegistration;
use search_application::source_registration::{
    CompleteDesiredRegistrations, RegistrationNamespace, RegistrationSetRevision,
    SourceRegistration, SourceRegistrationCatalog, SyntheticHostRegistrationAuthority,
    SyntheticRegistrationLedger,
};

pub struct RemoteCatalogFixture {
    catalog: SourceRegistrationCatalog,
    host: Arc<SyntheticHostRegistrationAuthority>,
    remote_revision: AtomicU64,
}

impl RemoteCatalogFixture {
    pub async fn start_complete(
        remotes: Vec<RemoteSourceRegistration>,
    ) -> Result<Self, SearchError> {
        let host = Arc::new(SyntheticHostRegistrationAuthority::new());
        let ledger = Arc::new(SyntheticRegistrationLedger::with_host(host));
        Self::start_complete_with_ledger(ledger, remotes).await
    }

    pub async fn start_complete_with_ledger(
        ledger: Arc<SyntheticRegistrationLedger>,
        remotes: Vec<RemoteSourceRegistration>,
    ) -> Result<Self, SearchError> {
        let host = ledger.host_for_synthetic();
        let document =
            match CompleteDesiredRegistrations::capture(&*host, RegistrationNamespace::Document)
                .await
            {
                Ok(snapshot) => snapshot,
                Err(_) => {
                    host.publish(
                        RegistrationNamespace::Document,
                        RegistrationSetRevision::new(1)?,
                        Vec::new(),
                    )?;
                    CompleteDesiredRegistrations::capture(&*host, RegistrationNamespace::Document)
                        .await?
                }
            };
        let next_revision = match CompleteDesiredRegistrations::capture(
            &*host,
            RegistrationNamespace::Remote,
        )
        .await
        {
            Ok(snapshot) => snapshot
                .deployment_revision()
                .get()
                .checked_add(1)
                .ok_or_else(revision_overflow)?,
            Err(_) => 1,
        };
        host.publish(
            RegistrationNamespace::Remote,
            RegistrationSetRevision::new(next_revision)?,
            remotes
                .into_iter()
                .map(SourceRegistration::Remote)
                .collect(),
        )?;
        let remote =
            CompleteDesiredRegistrations::capture(&*host, RegistrationNamespace::Remote).await?;
        let catalog = SourceRegistrationCatalog::try_new(ledger, &document, &remote).await?;
        Ok(Self {
            catalog,
            host,
            remote_revision: AtomicU64::new(next_revision),
        })
    }

    pub async fn publish_complete(
        &self,
        remotes: Vec<RemoteSourceRegistration>,
    ) -> Result<(), SearchError> {
        let prior = self
            .remote_revision
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |revision| {
                revision.checked_add(1)
            })
            .map_err(|_| revision_overflow())?;
        self.host.publish(
            RegistrationNamespace::Remote,
            RegistrationSetRevision::new(prior + 1)?,
            remotes
                .into_iter()
                .map(SourceRegistration::Remote)
                .collect(),
        )?;
        let desired =
            CompleteDesiredRegistrations::capture(&*self.host, RegistrationNamespace::Remote)
                .await?;
        self.catalog.replace_checked(&desired).await
    }
}

impl Deref for RemoteCatalogFixture {
    type Target = SourceRegistrationCatalog;

    fn deref(&self) -> &Self::Target {
        &self.catalog
    }
}

fn revision_overflow() -> SearchError {
    SearchError::InvalidRequest("test registration revision overflow".into())
}

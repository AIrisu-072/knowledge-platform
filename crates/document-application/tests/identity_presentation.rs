use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use document_application::{
    IdentityPresentation, IdentityPresentationResolution, IdentityPresentationResolutionError,
    IdentityPresentationResolver, IdentityPresentationService, IdentityRef,
};
use document_domain::{PolicySubject, PolicySubjectKind};

#[derive(Clone)]
struct RecordingResolver {
    calls: Arc<AtomicUsize>,
    requested: Arc<Mutex<Vec<Vec<IdentityRef>>>>,
    response: Result<Vec<IdentityPresentation>, IdentityPresentationResolutionError>,
}

impl RecordingResolver {
    fn returning(
        response: Result<Vec<IdentityPresentation>, IdentityPresentationResolutionError>,
    ) -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            requested: Arc::new(Mutex::new(Vec::new())),
            response,
        }
    }
}

impl IdentityPresentationResolver for RecordingResolver {
    fn resolve_batch<'a>(
        &'a self,
        refs: &'a [IdentityRef],
    ) -> Pin<
        Box<
            dyn Future<Output = Result<Vec<IdentityPresentation>, IdentityPresentationResolutionError>>
                + Send
                + 'a,
        >,
    > {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requested.lock().unwrap().push(refs.to_vec());
        let response = self.response.clone();
        Box::pin(async move { response })
    }
}

#[tokio::test]
async fn batch_resolution_deduplicates_refs_and_preserves_fallback_status() {
    let principal = IdentityRef::from_policy_subject(
        &PolicySubject::new(PolicySubjectKind::Principal, "directory", "alice").unwrap(),
    );
    let group = IdentityRef::from_policy_subject(
        &PolicySubject::new(PolicySubjectKind::Group, "directory", "reviewers").unwrap(),
    );
    let resolver = RecordingResolver::returning(Ok(vec![
        IdentityPresentation {
            reference: principal.clone(),
            display_name: Some("Alice Chen".into()),
            secondary_text: Some("Quality".into()),
            resolution: IdentityPresentationResolution::Resolved,
        },
        IdentityPresentation {
            reference: group.clone(),
            display_name: Some("must be discarded for notFound".into()),
            secondary_text: Some("must also be discarded".into()),
            resolution: IdentityPresentationResolution::NotFound,
        },
    ]));

    let resolved = IdentityPresentationService::resolve_batch(
        &resolver,
        &[principal.clone(), group.clone(), principal.clone()],
    )
    .await;

    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        resolver.requested.lock().unwrap().as_slice(),
        &[vec![principal.clone(), group.clone()]]
    );
    assert_eq!(resolved.len(), 3);
    assert_eq!(resolved[0].display_name.as_deref(), Some("Alice Chen"));
    assert_eq!(resolved[1].reference, group);
    assert_eq!(resolved[1].resolution, IdentityPresentationResolution::NotFound);
    assert_eq!(resolved[1].display_name, None);
    assert_eq!(resolved[1].secondary_text, None);
    assert_eq!(resolved[2], resolved[0]);
}

#[tokio::test]
async fn unavailable_or_missing_resolver_results_fall_back_without_losing_identity_refs() {
    let principal = IdentityRef::from_policy_subject(
        &PolicySubject::new(PolicySubjectKind::Role, "directory", "document-editor").unwrap(),
    );
    let resolver = RecordingResolver::returning(Err(
        IdentityPresentationResolutionError::Unavailable,
    ));

    let resolved = IdentityPresentationService::resolve_batch(&resolver, std::slice::from_ref(&principal)).await;

    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].reference, principal);
    assert_eq!(resolved[0].display_name, None);
    assert_eq!(resolved[0].secondary_text, None);
    assert_eq!(resolved[0].resolution, IdentityPresentationResolution::Unavailable);
}

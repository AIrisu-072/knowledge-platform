use search_core::binding::{
    BindingMode, RepresentationBinding, RevalidationMarker, SessionBindingSet,
};
use search_core::id::{BindingId, LogicalResourceId, RepresentationId, SourceId};
use time::OffsetDateTime;
use uuid::Uuid;

fn binding(mode: BindingMode, representation: u128) -> RepresentationBinding {
    RepresentationBinding::new(
        BindingId::from_uuid(Uuid::from_u128(1)),
        LogicalResourceId::from_uuid(Uuid::from_u128(2)),
        RepresentationId::from_uuid(Uuid::from_u128(representation)),
        SourceId::from_uuid(Uuid::from_u128(4)),
        mode,
        OffsetDateTime::from_unix_timestamp(100).unwrap(),
    )
}

#[test]
fn registry_updates_do_not_silently_replace_an_existing_binding() {
    let mut session = SessionBindingSet::default();
    let first = binding(BindingMode::SessionSnapshot, 3);
    let newer = binding(BindingMode::SessionSnapshot, 5);
    assert_eq!(
        session
            .bind_if_absent(first.clone())
            .unwrap()
            .representation_ref,
        first.representation_ref
    );
    assert!(session.bind_if_absent(newer).is_err());
    assert_eq!(
        session.get(first.binding_id).unwrap().representation_ref,
        first.representation_ref
    );
}

#[test]
fn live_reference_requires_current_revalidation_marker() {
    let mut live = binding(BindingMode::LiveReference, 3);
    assert_eq!(live.revalidation_marker, RevalidationMarker::Required);
    assert!(live.validate().is_ok());
    live.revalidation_marker = RevalidationMarker::NotRequired;
    assert!(live.validate().is_err());
    let mut session = SessionBindingSet::default();
    assert!(session.bind_if_absent(live).is_err());
}

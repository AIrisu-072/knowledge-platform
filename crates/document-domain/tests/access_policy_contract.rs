use document_domain::{
    Action, PolicyGrant, PolicyMode, PolicySubject, PolicySubjectKind, evaluate_policy,
    nearest_explicit_policy,
};

fn principal(id: &str) -> PolicySubject {
    PolicySubject::new(PolicySubjectKind::Principal, "windows", id).unwrap()
}

fn group(issuer: &str, id: &str) -> PolicySubject {
    PolicySubject::new(PolicySubjectKind::Group, issuer, id).unwrap()
}

#[test]
fn nearest_policy_replaces_not_unions() {
    let actor = principal("alice");
    let parent = PolicyMode::Explicit(vec![
        PolicyGrant::new(actor.clone(), [Action::Read]).unwrap(),
    ]);
    let child = PolicyMode::Explicit(vec![
        PolicyGrant::new(actor.clone(), [Action::Write]).unwrap(),
    ]);
    let effective = nearest_explicit_policy([&child, &parent]).unwrap();
    assert!(evaluate_policy(
        std::slice::from_ref(&actor),
        effective,
        &[Action::Write]
    ));
    assert!(!evaluate_policy(&[actor], effective, &[Action::Read]));
}

#[test]
fn explicit_empty_is_not_inherit() {
    let actor = principal("alice");
    assert!(PolicyMode::validate_explicit(&[]).is_err());
    assert!(!evaluate_policy(
        std::slice::from_ref(&actor),
        &PolicyMode::Explicit(vec![]),
        &[Action::Read]
    ));
    assert!(!evaluate_policy(
        &[actor],
        &PolicyMode::Inherit,
        &[Action::Read]
    ));
}

#[test]
fn issuer_separates_groups() {
    let allowed = group("tenant-a", "editors");
    let same_name_other_issuer = group("tenant-b", "editors");
    let mode = PolicyMode::Explicit(vec![PolicyGrant::new(allowed, [Action::Read]).unwrap()]);
    assert!(!evaluate_policy(
        &[same_name_other_issuer],
        &mode,
        &[Action::Read]
    ));
}

#[test]
fn administer_does_not_imply_read() {
    let actor = principal("alice");
    let mode = PolicyMode::Explicit(vec![
        PolicyGrant::new(actor.clone(), [Action::Administer]).unwrap(),
    ]);
    assert!(evaluate_policy(
        std::slice::from_ref(&actor),
        &mode,
        &[Action::Administer]
    ));
    assert!(!evaluate_policy(&[actor], &mode, &[Action::Read]));
}

#[test]
fn multiple_grants_can_satisfy_all_required_actions() {
    let actor = principal("alice");
    let team = group("directory", "writers");
    let mode = PolicyMode::Explicit(vec![
        PolicyGrant::new(actor.clone(), [Action::Read]).unwrap(),
        PolicyGrant::new(team.clone(), [Action::Write]).unwrap(),
    ]);
    assert!(evaluate_policy(
        &[actor, team],
        &mode,
        &[Action::Read, Action::Write]
    ));
}

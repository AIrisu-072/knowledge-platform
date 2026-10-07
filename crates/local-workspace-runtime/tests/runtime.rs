#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{FileExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use local_workspace_runtime::{
    Availability, ContextRef, DirectorySelection, EntryKind, LocalRef, LocalWorkspaceRuntime,
    RuntimeErrorCode, RuntimeErrorReason, RuntimeOptions, RuntimeWorkspaceOutcome, WorkspaceView,
};
use tempfile::TempDir;

fn op() -> String {
    uuid::Uuid::new_v4().to_string()
}

struct Fixture {
    temp: TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            temp: tempfile::tempdir().unwrap(),
        }
    }
    fn state(&self) -> PathBuf {
        self.temp.path().join("state")
    }
    fn folder(&self, name: &str) -> PathBuf {
        let path = self.temp.path().join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }
    fn open(&self) -> LocalWorkspaceRuntime {
        LocalWorkspaceRuntime::open(self.state()).unwrap()
    }
    fn open_with(&self, options: RuntimeOptions) -> LocalWorkspaceRuntime {
        LocalWorkspaceRuntime::open_with_options(self.state(), options).unwrap()
    }
}

fn ctx(view: &WorkspaceView) -> ContextRef {
    ContextRef {
        workspace_id: view.workspace_id.clone(),
        effective_context_revision: view.effective_context_revision.clone(),
    }
}

fn select(rt: &LocalWorkspaceRuntime, view: &WorkspaceView, path: &Path) -> DirectorySelection {
    let ticket = rt.begin_directory_selection(&ctx(view)).unwrap();
    rt.finish_directory_selection(ticket, Some(path.to_path_buf()))
        .unwrap()
        .expect("selection")
}

fn attach(
    rt: &LocalWorkspaceRuntime,
    view: &WorkspaceView,
    path: &Path,
) -> (WorkspaceView, String) {
    let selection = select(rt, view, path);
    let attached = rt.attach_directory(&ctx(view), &selection, &op()).unwrap();
    (attached.workspace, attached.receipt.binding_id)
}

fn local(binding: &str, locator: &[&str]) -> LocalRef {
    LocalRef {
        binding_id: binding.to_owned(),
        locator: locator.iter().map(|s| (*s).to_owned()).collect(),
    }
}

fn code<T: std::fmt::Debug>(
    result: Result<T, local_workspace_runtime::RuntimeError>,
) -> RuntimeErrorCode {
    result.expect_err("expected a runtime error").code
}

fn new_workspace(rt: &LocalWorkspaceRuntime, name: &str) -> WorkspaceView {
    rt.create_workspace(name, &op()).unwrap().workspace
}

#[test]
fn capabilities_report_local_runtime_without_multi_window_or_sidecar() {
    let fixture = Fixture::new();
    let rt = fixture.open();
    let caps = rt.capabilities(true);
    assert_eq!(caps.local_resources, Availability::Available);
    assert_eq!(caps.managed_workspace, Availability::Available);
    assert_eq!(caps.native_directory_picker, Availability::Available);
    assert!(!caps.multi_window && !caps.sidecar);
    assert_eq!(
        rt.capabilities(false).native_directory_picker,
        Availability::Unavailable
    );
}

#[test]
fn create_workspace_makes_managed_root_independent_of_logical_name() {
    let fixture = Fixture::new();
    let rt = fixture.open();
    let created = rt.create_workspace("見積/../x", &op()).unwrap();
    let view = &created.workspace;
    assert_eq!(view.name, "見積/../x");
    assert_eq!(view.scope, "principal_device_local");
    assert_eq!(view.bindings.len(), 1);
    assert_eq!(view.bindings[0].binding_id, view.managed_binding_id);
    assert!(view.bindings[0].available);
    let managed: Vec<_> = fs::read_dir(fixture.state().join("managed"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(managed, vec![view.managed_binding_id.clone()]);
    assert!(!fixture.temp.path().join("x").exists());
    assert_eq!(rt.list_workspaces().unwrap(), vec![view.clone()]);
    // Empty managed root is listable and writable through relative locators.
    let page = rt
        .list_entries(&ctx(view), &local(&view.managed_binding_id, &[]), None)
        .unwrap();
    assert!(page.entries.is_empty());
}

#[test]
fn create_workspace_retry_by_operation_id_never_creates_another_root() {
    let fixture = Fixture::new();
    let rt = fixture.open();
    let id = op();
    let first = rt.create_workspace("A", &id).unwrap();
    let second = rt.create_workspace("A", &id).unwrap();
    assert_eq!(first.receipt, second.receipt);
    assert_eq!(rt.list_workspaces().unwrap().len(), 1);
    assert_eq!(
        fs::read_dir(fixture.state().join("managed"))
            .unwrap()
            .count(),
        1
    );
    assert_eq!(
        code(rt.create_workspace("B", &id)),
        RuntimeErrorCode::Conflict
    );
    match rt.recover_workspace(&id) {
        RuntimeWorkspaceOutcome::Ready { receipt } => assert_eq!(receipt, first.receipt),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(
        rt.recover_workspace(&op()),
        RuntimeWorkspaceOutcome::NotFound
    );
}

#[test]
fn restart_restores_workspaces_bindings_and_retry_receipts() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    fs::write(folder.join("a.txt"), b"alpha").unwrap();
    let create_id = op();
    let (view, binding) = {
        let rt = fixture.open();
        let view = rt
            .create_workspace("restore", &create_id)
            .unwrap()
            .workspace;
        attach(&rt, &view, &folder)
    };
    let rt = fixture.open();
    let restored = rt.list_workspaces().unwrap();
    assert_eq!(restored, vec![view.clone()]);
    let page = rt
        .list_entries(&ctx(&view), &local(&binding, &[]), None)
        .unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].name, "a.txt");
    assert!(matches!(
        rt.recover_workspace(&create_id),
        RuntimeWorkspaceOutcome::Ready { .. }
    ));
}

#[test]
fn second_runtime_instance_on_same_state_is_rejected() {
    let fixture = Fixture::new();
    let _first = fixture.open();
    let error = LocalWorkspaceRuntime::open(fixture.state()).unwrap_err();
    assert_eq!(error.code, RuntimeErrorCode::Unavailable);
    assert_eq!(error.reason, Some(RuntimeErrorReason::InstanceLocked));
}

#[test]
fn corrupted_registry_fails_closed_without_being_reset() {
    let fixture = Fixture::new();
    drop(fixture.open());
    let registry = fixture.state().join("registry.json");
    fs::write(&registry, b"{not json").unwrap();
    let error = LocalWorkspaceRuntime::open(fixture.state()).unwrap_err();
    assert_eq!(error.code, RuntimeErrorCode::Unavailable);
    assert_eq!(error.reason, Some(RuntimeErrorReason::RegistryUnreadable));
    assert_eq!(fs::read(&registry).unwrap(), b"{not json");
}

#[test]
fn rename_changes_only_the_logical_name() {
    let fixture = Fixture::new();
    let rt = fixture.open();
    let view = new_workspace(&rt, "old");
    let renamed = rt.rename_workspace(&ctx(&view), " new ", &op()).unwrap();
    assert_eq!(renamed.name, "new");
    assert_eq!(renamed.managed_binding_id, view.managed_binding_id);
    assert_eq!(
        renamed.effective_context_revision,
        view.effective_context_revision
    );
    assert!(
        fixture
            .state()
            .join("managed")
            .join(&view.managed_binding_id)
            .is_dir()
    );
    assert_eq!(
        code(rt.rename_workspace(&ctx(&view), "\n", &op())),
        RuntimeErrorCode::InvalidLocator
    );
}

#[test]
fn context_must_name_a_current_workspace_revision() {
    let fixture = Fixture::new();
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let mut stale = ctx(&view);
    stale.effective_context_revision = "r999".into();
    assert_eq!(
        code(rt.list_entries(&stale, &local(&view.managed_binding_id, &[]), None)),
        RuntimeErrorCode::StaleContext
    );
    let unknown = ContextRef {
        workspace_id: "w_unknown".into(),
        effective_context_revision: view.effective_context_revision.clone(),
    };
    assert_eq!(
        code(rt.list_entries(&unknown, &local(&view.managed_binding_id, &[]), None)),
        RuntimeErrorCode::NotFound
    );
}

#[test]
fn picker_cancel_and_double_open_change_nothing() {
    let fixture = Fixture::new();
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let ticket = rt.begin_directory_selection(&ctx(&view)).unwrap();
    assert_eq!(
        code(rt.begin_directory_selection(&ctx(&view))),
        RuntimeErrorCode::Conflict
    );
    assert_eq!(rt.finish_directory_selection(ticket, None).unwrap(), None);
    assert_eq!(rt.list_workspaces().unwrap(), vec![view.clone()]);
    // The picker is free again after cancellation.
    let ticket = rt.begin_directory_selection(&ctx(&view)).unwrap();
    assert_eq!(rt.finish_directory_selection(ticket, None).unwrap(), None);
}

#[test]
fn selection_is_single_use_and_bound_to_its_workspace() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    let rt = fixture.open();
    let a = new_workspace(&rt, "a");
    let b = new_workspace(&rt, "b");
    let selection = select(&rt, &a, &folder);
    assert!(selection.selection_id.len() >= 16);
    assert_eq!(
        code(rt.attach_directory(&ctx(&b), &selection, &op())),
        RuntimeErrorCode::NotFound
    );
    let attached = rt.attach_directory(&ctx(&a), &selection, &op()).unwrap();
    assert_eq!(attached.workspace.bindings.len(), 2);
    assert_eq!(
        code(rt.attach_directory(&ctx(&attached.workspace), &selection, &op())),
        RuntimeErrorCode::NotFound
    );
}

#[test]
fn selection_expires() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    let rt = fixture.open_with(RuntimeOptions {
        selection_ttl: Duration::from_millis(1),
        ..RuntimeOptions::default()
    });
    let view = new_workspace(&rt, "w");
    let selection = select(&rt, &view, &folder);
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(
        code(rt.attach_directory(&ctx(&view), &selection, &op())),
        RuntimeErrorCode::NotFound
    );
}

#[test]
fn attach_rejects_duplicates_and_the_protected_state_root() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let (view, _) = attach(&rt, &view, &folder);
    let again = select(&rt, &view, &folder);
    let error = rt.attach_directory(&ctx(&view), &again, &op()).unwrap_err();
    assert_eq!(error.code, RuntimeErrorCode::Conflict);
    assert_eq!(error.reason, Some(RuntimeErrorReason::AlreadyBound));
    for protected in [fixture.state(), fixture.state().join("managed")] {
        let ticket = rt.begin_directory_selection(&ctx(&view)).unwrap();
        let error = rt
            .finish_directory_selection(ticket, Some(protected))
            .unwrap_err();
        assert_eq!(error.code, RuntimeErrorCode::Denied);
        assert_eq!(error.reason, Some(RuntimeErrorReason::ProtectedLocation));
    }
}

#[test]
fn attach_retry_by_operation_id_returns_the_same_binding() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let selection = select(&rt, &view, &folder);
    let id = op();
    let first = rt.attach_directory(&ctx(&view), &selection, &id).unwrap();
    // Lost response: same operation and selection under the original context.
    let second = rt.attach_directory(&ctx(&view), &selection, &id).unwrap();
    assert_eq!(first.receipt, second.receipt);
    assert_eq!(second.workspace.bindings.len(), 2);
}

#[test]
fn detach_removes_only_the_explicit_reference() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    fs::write(folder.join("keep.txt"), b"keep").unwrap();
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let (view, binding) = attach(&rt, &view, &folder);
    let page = rt
        .list_entries(&ctx(&view), &local(&binding, &[]), None)
        .unwrap();
    let handle = rt
        .open_read(
            &ctx(&view),
            &local(&binding, &["keep.txt"]),
            &page.entries[0].file_identity,
        )
        .unwrap();
    let managed_error = rt
        .detach_directory(&ctx(&view), &view.managed_binding_id, &op())
        .unwrap_err();
    assert_eq!(managed_error.code, RuntimeErrorCode::Denied);
    assert_eq!(
        managed_error.reason,
        Some(RuntimeErrorReason::ManagedBinding)
    );
    let detach_id = op();
    let after = rt
        .detach_directory(&ctx(&view), &binding, &detach_id)
        .unwrap();
    assert_eq!(after.bindings.len(), 1);
    assert_ne!(
        after.effective_context_revision,
        view.effective_context_revision
    );
    assert_eq!(fs::read(folder.join("keep.txt")).unwrap(), b"keep");
    // Retry of the same detach is idempotent; old contexts and handles are dead.
    assert_eq!(
        rt.detach_directory(&ctx(&view), &binding, &detach_id)
            .unwrap(),
        after
    );
    assert_eq!(
        code(rt.read_file(&ctx(&view), &handle.read_handle_id, 0, 4)),
        RuntimeErrorCode::StaleContext
    );
    assert_eq!(
        code(rt.read_file(&ctx(&after), &handle.read_handle_id, 0, 4)),
        RuntimeErrorCode::NotFound
    );
    assert_eq!(
        code(rt.list_entries(&ctx(&after), &local(&binding, &[]), None)),
        RuntimeErrorCode::NotFound
    );
}

#[test]
fn traversal_and_malformed_locators_are_rejected_before_io() {
    let fixture = Fixture::new();
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let m = &view.managed_binding_id;
    for locator in [
        vec![".."],
        vec!["a", ".."],
        vec!["."],
        vec![""],
        vec!["a/b"],
        vec!["/etc"],
        vec!["C:\\Windows"],
        vec!["con"],
        vec!["x:stream"],
    ] {
        assert_eq!(
            code(rt.list_entries(&ctx(&view), &local(m, &locator), None)),
            RuntimeErrorCode::InvalidLocator,
            "{locator:?}"
        );
        assert_eq!(
            code(rt.create_file(&ctx(&view), &local(m, &locator), "n.txt", b"x", &op())),
            RuntimeErrorCode::InvalidLocator,
            "{locator:?}"
        );
    }
    assert_eq!(
        code(rt.create_file(&ctx(&view), &local(m, &[]), "../escape.txt", b"x", &op())),
        RuntimeErrorCode::InvalidLocator
    );
    assert!(!fixture.state().join("managed").join("escape.txt").exists());
}

#[test]
fn listing_pages_are_bounded_stable_and_report_omissions() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    for i in 0..150 {
        fs::write(folder.join(format!("f{i:03}.txt")), b"x").unwrap();
    }
    fs::create_dir(folder.join("sub")).unwrap();
    symlink("/", folder.join("link-root")).unwrap();
    fs::write(folder.join("CON"), b"reserved on windows").unwrap();
    fs::write(folder.join("back\\slash"), b"x").unwrap();
    let fifo =
        std::ffi::CString::new(folder.join("pipe").into_os_string().into_encoded_bytes()).unwrap();
    // SAFETY: valid NUL-terminated path for a test FIFO.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let (view, binding) = attach(&rt, &view, &folder);
    let first = rt
        .list_entries(&ctx(&view), &local(&binding, &[]), None)
        .unwrap();
    assert_eq!(first.entries.len(), 100);
    let cursor = first.next_cursor.clone().expect("next page");
    assert!(!cursor.contains(".txt"), "cursor must not expose raw names");
    let second = rt
        .list_entries(&ctx(&view), &local(&binding, &[]), Some(&cursor))
        .unwrap();
    assert!(second.next_cursor.is_none());
    let mut names: Vec<String> = first
        .entries
        .iter()
        .chain(&second.entries)
        .map(|e| e.name.clone())
        .collect();
    assert_eq!(names.len(), 151);
    names.dedup();
    assert_eq!(names.len(), 151);
    assert!(names.contains(&"sub".to_owned()));
    for hidden in ["link-root", "CON", "back\\slash", "pipe"] {
        assert!(!names.contains(&hidden.to_owned()), "{hidden}");
    }
    assert_eq!(first.omitted_count + second.omitted_count, 4);
    let sub = first
        .entries
        .iter()
        .chain(&second.entries)
        .find(|e| e.name == "sub")
        .unwrap();
    assert_eq!(sub.kind, EntryKind::Directory);
    assert_eq!(sub.locator, vec!["sub".to_owned()]);
    // A cursor from another directory is rejected.
    assert_eq!(
        code(rt.list_entries(&ctx(&view), &local(&binding, &["sub"]), Some(&cursor))),
        RuntimeErrorCode::InvalidLocator
    );
}

#[test]
fn symlinks_never_escape_the_binding() {
    let fixture = Fixture::new();
    let outside = fixture.folder("outside");
    fs::write(outside.join("secret.txt"), b"secret").unwrap();
    let folder = fixture.folder("folder");
    symlink(&outside, folder.join("dirlink")).unwrap();
    symlink(outside.join("secret.txt"), folder.join("filelink.txt")).unwrap();
    fs::create_dir(folder.join("real")).unwrap();
    symlink("../../outside", folder.join("real").join("up")).unwrap();
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let (view, binding) = attach(&rt, &view, &folder);
    let c = ctx(&view);
    assert_eq!(
        code(rt.list_entries(&c, &local(&binding, &["dirlink"]), None)),
        RuntimeErrorCode::Denied
    );
    assert_eq!(
        code(rt.list_entries(&c, &local(&binding, &["real", "up"]), None)),
        RuntimeErrorCode::Denied
    );
    assert_eq!(
        code(rt.open_read(&c, &local(&binding, &["dirlink", "secret.txt"]), "x")),
        RuntimeErrorCode::Denied
    );
    assert_eq!(
        code(rt.open_read(&c, &local(&binding, &["filelink.txt"]), "x")),
        RuntimeErrorCode::Denied
    );
    assert_eq!(
        code(rt.create_file(&c, &local(&binding, &["dirlink"]), "new.txt", b"x", &op())),
        RuntimeErrorCode::Denied
    );
    // A dangling symlink at the create target is not followed either.
    symlink(
        outside.join("created-through-link.txt"),
        folder.join("dangling.txt"),
    )
    .unwrap();
    assert_eq!(
        code(rt.create_file(&c, &local(&binding, &[]), "dangling.txt", b"x", &op())),
        RuntimeErrorCode::Conflict
    );
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
}

#[test]
fn replaced_or_symlinked_bound_root_becomes_unavailable() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let (view, binding) = attach(&rt, &view, &folder);
    fs::rename(&folder, fixture.temp.path().join("moved")).unwrap();
    fs::create_dir(&folder).unwrap();
    let error = rt
        .list_entries(&ctx(&view), &local(&binding, &[]), None)
        .unwrap_err();
    assert_eq!(error.code, RuntimeErrorCode::Unavailable);
    assert_eq!(error.reason, Some(RuntimeErrorReason::FolderReplaced));
    let listed = rt.list_workspaces().unwrap();
    let explicit = listed[0]
        .bindings
        .iter()
        .find(|b| b.binding_id == binding)
        .unwrap();
    assert!(!explicit.available);
    fs::remove_dir(&folder).unwrap();
    symlink(fixture.temp.path().join("moved"), &folder).unwrap();
    assert_eq!(
        code(rt.list_entries(&ctx(&view), &local(&binding, &[]), None)),
        RuntimeErrorCode::Unavailable
    );
}

#[test]
fn hardlinked_and_special_files_are_not_read() {
    let fixture = Fixture::new();
    let outside = fixture.folder("outside");
    fs::write(outside.join("secret.txt"), b"secret").unwrap();
    let folder = fixture.folder("folder");
    fs::hard_link(outside.join("secret.txt"), folder.join("hard.txt")).unwrap();
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let (view, binding) = attach(&rt, &view, &folder);
    let page = rt
        .list_entries(&ctx(&view), &local(&binding, &[]), None)
        .unwrap();
    let identity = page.entries[0].file_identity.clone();
    let error = rt
        .open_read(&ctx(&view), &local(&binding, &["hard.txt"]), &identity)
        .unwrap_err();
    assert_eq!(error.code, RuntimeErrorCode::Denied);
    assert_eq!(error.reason, Some(RuntimeErrorReason::LinkedFile));
}

#[test]
fn protected_state_root_is_hidden_and_denied_inside_an_ancestor_binding() {
    let fixture = Fixture::new();
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    // Binding the parent of the state root is allowed, but the runtime's own
    // registry and managed roots stay unreachable through it.
    let (view, binding) = attach(&rt, &view, fixture.temp.path());
    let page = rt
        .list_entries(&ctx(&view), &local(&binding, &[]), None)
        .unwrap();
    assert!(page.entries.iter().all(|e| e.name != "state"));
    assert_eq!(
        code(rt.list_entries(&ctx(&view), &local(&binding, &["state"]), None)),
        RuntimeErrorCode::Denied
    );
    assert_eq!(
        code(rt.open_read(
            &ctx(&view),
            &local(&binding, &["state", "registry.json"]),
            "x"
        )),
        RuntimeErrorCode::Denied
    );
}

#[test]
fn bounded_read_handles_follow_identity_ranges_and_lifetime() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    let body: Vec<u8> = (0..(1024 * 1024 + 10)).map(|i| (i % 251) as u8).collect();
    fs::write(folder.join("data.bin"), &body).unwrap();
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let (view, binding) = attach(&rt, &view, &folder);
    let c = ctx(&view);
    let r = local(&binding, &["data.bin"]);
    let identity = rt
        .list_entries(&c, &local(&binding, &[]), None)
        .unwrap()
        .entries[0]
        .file_identity
        .clone();
    assert_eq!(
        code(rt.open_read(&c, &r, "wrong-identity")),
        RuntimeErrorCode::Conflict
    );
    let handle = rt.open_read(&c, &r, &identity).unwrap();
    assert_eq!(handle.size_bytes, body.len() as u64);
    let page = rt
        .read_file(&c, &handle.read_handle_id, 0, 1024 * 1024)
        .unwrap();
    assert_eq!(page.bytes, body[..1024 * 1024]);
    assert!(!page.eof);
    assert_eq!(page.content_generation, handle.content_generation);
    let tail = rt
        .read_file(&c, &handle.read_handle_id, 1024 * 1024, 1024 * 1024)
        .unwrap();
    assert_eq!(tail.bytes, body[1024 * 1024..]);
    assert!(tail.eof);
    assert_eq!(
        code(rt.read_file(&c, &handle.read_handle_id, 0, 1024 * 1024 + 1)),
        RuntimeErrorCode::Limit
    );
    assert_eq!(
        code(rt.read_file(&c, &handle.read_handle_id, body.len() as u64 + 1, 1)),
        RuntimeErrorCode::InvalidLocator
    );
    // The snapshot is immutable even if the file changes afterwards.
    fs::write(folder.join("data.bin"), b"changed").unwrap();
    assert_eq!(
        rt.read_file(&c, &handle.read_handle_id, 0, 3)
            .unwrap()
            .bytes,
        body[..3]
    );
    rt.close_read(&c, &handle.read_handle_id).unwrap();
    assert_eq!(
        code(rt.read_file(&c, &handle.read_handle_id, 0, 1)),
        RuntimeErrorCode::NotFound
    );
    // Same path, new content: the old identity still matches the inode but
    // replacement by a different inode does not.
    fs::write(folder.join("next.bin"), b"next").unwrap();
    fs::rename(folder.join("next.bin"), folder.join("data.bin")).unwrap();
    assert_eq!(
        code(rt.open_read(&c, &r, &identity)),
        RuntimeErrorCode::Conflict
    );
}

#[test]
fn read_handle_count_bytes_and_lease_are_bounded() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    for i in 0..5 {
        fs::write(folder.join(format!("{i}.txt")), b"x").unwrap();
    }
    let rt = fixture.open_with(RuntimeOptions {
        handle_lease: Duration::from_millis(50),
        ..RuntimeOptions::default()
    });
    let view = new_workspace(&rt, "w");
    let (view, binding) = attach(&rt, &view, &folder);
    let c = ctx(&view);
    let page = rt.list_entries(&c, &local(&binding, &[]), None).unwrap();
    let mut handles = Vec::new();
    for entry in &page.entries[..4] {
        handles.push(
            rt.open_read(&c, &local(&binding, &[&entry.name]), &entry.file_identity)
                .unwrap(),
        );
    }
    let fifth = &page.entries[4];
    assert_eq!(
        code(rt.open_read(&c, &local(&binding, &[&fifth.name]), &fifth.file_identity)),
        RuntimeErrorCode::Limit
    );
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(
        code(rt.read_file(&c, &handles[0].read_handle_id, 0, 1)),
        RuntimeErrorCode::NotFound
    );
    // Expired leases free their slots.
    rt.open_read(&c, &local(&binding, &[&fifth.name]), &fifth.file_identity)
        .unwrap();
}

#[test]
fn oversized_files_are_not_snapshotted() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    let file = fs::File::create(folder.join("big.bin")).unwrap();
    file.set_len(8 * 1024 * 1024 + 1).unwrap();
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let (view, binding) = attach(&rt, &view, &folder);
    let entry = rt
        .list_entries(&ctx(&view), &local(&binding, &[]), None)
        .unwrap()
        .entries[0]
        .clone();
    assert_eq!(
        code(rt.open_read(
            &ctx(&view),
            &local(&binding, &["big.bin"]),
            &entry.file_identity
        )),
        RuntimeErrorCode::Limit
    );
}

#[test]
fn restart_never_resurrects_read_handles() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    fs::write(folder.join("a.txt"), b"a").unwrap();
    let (view, handle) = {
        let rt = fixture.open();
        let view = new_workspace(&rt, "w");
        let (view, binding) = attach(&rt, &view, &folder);
        let entry = rt
            .list_entries(&ctx(&view), &local(&binding, &[]), None)
            .unwrap()
            .entries[0]
            .clone();
        let handle = rt
            .open_read(
                &ctx(&view),
                &local(&binding, &["a.txt"]),
                &entry.file_identity,
            )
            .unwrap();
        (view, handle)
    };
    let rt = fixture.open();
    assert_eq!(
        code(rt.read_file(&ctx(&view), &handle.read_handle_id, 0, 1)),
        RuntimeErrorCode::NotFound
    );
}

#[test]
fn concurrent_same_inode_writer_never_yields_a_mixed_snapshot() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    let size = 1024 * 1024;
    fs::write(folder.join("hot.bin"), vec![b'A'; size]).unwrap();
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let (view, binding) = attach(&rt, &view, &folder);
    let c = ctx(&view);
    let entry = rt
        .list_entries(&c, &local(&binding, &[]), None)
        .unwrap()
        .entries[0]
        .clone();
    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let stop = Arc::clone(&stop);
        let path = folder.join("hot.bin");
        std::thread::spawn(move || {
            let file = fs::OpenOptions::new().write(true).open(path).unwrap();
            let mut fill = b'B';
            while !stop.load(Ordering::Relaxed) {
                let chunk = vec![fill; 64 * 1024];
                for offset in (0..size).step_by(chunk.len()) {
                    file.write_at(&chunk, offset as u64).unwrap();
                }
                fill = if fill == b'A' { b'B' } else { b'A' };
            }
        })
    };
    let (mut conflicts, mut snapshots) = (0, 0);
    for _ in 0..200 {
        match rt.open_read(&c, &local(&binding, &["hot.bin"]), &entry.file_identity) {
            Ok(handle) => {
                snapshots += 1;
                let mut bytes = Vec::new();
                for offset in (0..size as u64).step_by(1024 * 1024) {
                    bytes.extend(
                        rt.read_file(&c, &handle.read_handle_id, offset, 1024 * 1024)
                            .unwrap()
                            .bytes,
                    );
                }
                assert!(
                    bytes.iter().all(|b| *b == bytes[0]),
                    "mixed snapshot returned"
                );
                rt.close_read(&c, &handle.read_handle_id).unwrap();
            }
            Err(error) => {
                assert_eq!(error.code, RuntimeErrorCode::Conflict);
                conflicts += 1;
            }
        }
    }
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();
    assert!(
        conflicts > 0,
        "the writer was never observed ({snapshots} snapshots)"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn writer_that_reopens_per_version_is_excluded_during_capture() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    let size = 1024 * 1024;
    fs::write(folder.join("cycle.bin"), vec![b'A'; size]).unwrap();
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let (view, binding) = attach(&rt, &view, &folder);
    let c = ctx(&view);
    let entry = rt
        .list_entries(&c, &local(&binding, &[]), None)
        .unwrap()
        .entries[0]
        .clone();
    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let stop = Arc::clone(&stop);
        let path = folder.join("cycle.bin");
        std::thread::spawn(move || {
            let mut fill = b'B';
            while !stop.load(Ordering::Relaxed) {
                // Opening for write blocks while the broker holds its read
                // lease, so a capture can never observe a half-written version.
                let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
                let chunk = vec![fill; 64 * 1024];
                for offset in (0..size).step_by(chunk.len()) {
                    file.write_at(&chunk, offset as u64).unwrap();
                    std::thread::yield_now();
                }
                drop(file);
                std::thread::sleep(Duration::from_millis(1));
                fill = if fill == b'A' { b'B' } else { b'A' };
            }
        })
    };
    let mut snapshots = 0;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while snapshots < 10 && std::time::Instant::now() < deadline {
        match rt.open_read(&c, &local(&binding, &["cycle.bin"]), &entry.file_identity) {
            Ok(handle) => {
                snapshots += 1;
                let bytes = rt
                    .read_file(&c, &handle.read_handle_id, 0, 1024 * 1024)
                    .unwrap()
                    .bytes;
                assert_eq!(bytes.len(), size);
                assert!(
                    bytes.iter().all(|b| *b == bytes[0]),
                    "mixed snapshot returned"
                );
                rt.close_read(&c, &handle.read_handle_id).unwrap();
            }
            Err(error) => {
                assert_eq!(error.code, RuntimeErrorCode::Conflict);
                std::thread::sleep(Duration::from_micros(300));
            }
        }
    }
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();
    assert!(
        snapshots > 0,
        "safe capture never succeeded between writers"
    );
}

#[test]
fn create_file_is_exclusive_bounded_and_idempotent_by_operation() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    fs::create_dir(folder.join("sub")).unwrap();
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let (view, binding) = attach(&rt, &view, &folder);
    let c = ctx(&view);
    let id = op();
    let receipt = rt
        .create_file(
            &c,
            &local(&binding, &["sub"]),
            "メモ.txt",
            "本文".as_bytes(),
            &id,
        )
        .unwrap();
    assert_eq!(
        receipt.r#ref.locator,
        vec!["sub".to_owned(), "メモ.txt".to_owned()]
    );
    assert_eq!(receipt.size_bytes, "本文".len() as u64);
    assert_eq!(receipt.sha256.len(), 64);
    assert_eq!(
        fs::read(folder.join("sub").join("メモ.txt")).unwrap(),
        "本文".as_bytes()
    );
    // Lost response: the same operation returns the stored receipt.
    assert_eq!(
        rt.create_file(
            &c,
            &local(&binding, &["sub"]),
            "メモ.txt",
            "本文".as_bytes(),
            &id
        )
        .unwrap(),
        receipt
    );
    // Same operation ID with different content is a mismatch, not a new write.
    let mismatch = rt
        .create_file(&c, &local(&binding, &["sub"]), "メモ.txt", b"other", &id)
        .unwrap_err();
    assert_eq!(mismatch.code, RuntimeErrorCode::Conflict);
    assert_eq!(mismatch.reason, Some(RuntimeErrorReason::OperationMismatch));
    // A new operation never overwrites an existing file.
    let exists = rt
        .create_file(&c, &local(&binding, &["sub"]), "メモ.txt", b"other", &op())
        .unwrap_err();
    assert_eq!(exists.reason, Some(RuntimeErrorReason::AlreadyExists));
    assert_eq!(
        fs::read(folder.join("sub").join("メモ.txt")).unwrap(),
        "本文".as_bytes()
    );
    let too_big = vec![0u8; 8 * 1024 * 1024 + 1];
    assert_eq!(
        code(rt.create_file(&c, &local(&binding, &[]), "big.bin", &too_big, &op())),
        RuntimeErrorCode::Limit
    );
    assert!(!folder.join("big.bin").exists());
    // The created file is immediately readable through its receipt identity.
    let handle = rt
        .open_read(&c, &receipt.r#ref, &receipt.file_identity)
        .unwrap();
    assert_eq!(
        rt.read_file(&c, &handle.read_handle_id, 0, 1024)
            .unwrap()
            .bytes,
        "本文".as_bytes()
    );
}

#[test]
fn create_retry_after_restart_returns_receipt_and_detects_replacement() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    let id = op();
    let (view, binding, receipt) = {
        let rt = fixture.open();
        let view = new_workspace(&rt, "w");
        let (view, binding) = attach(&rt, &view, &folder);
        let receipt = rt
            .create_file(&ctx(&view), &local(&binding, &[]), "a.txt", b"a", &id)
            .unwrap();
        (view, binding, receipt)
    };
    let rt = fixture.open();
    assert_eq!(
        rt.create_file(&ctx(&view), &local(&binding, &[]), "a.txt", b"a", &id)
            .unwrap(),
        receipt
    );
    fs::remove_file(folder.join("a.txt")).unwrap();
    fs::write(folder.join("a.txt"), b"a").unwrap();
    let error = rt
        .create_file(&ctx(&view), &local(&binding, &[]), "a.txt", b"a", &id)
        .unwrap_err();
    assert_eq!(error.code, RuntimeErrorCode::Conflict);
}

#[test]
fn parent_swapped_for_symlink_during_creation_never_writes_outside() {
    let fixture = Fixture::new();
    let outside = fixture.folder("outside");
    let folder = fixture.folder("folder");
    fs::create_dir(folder.join("sub")).unwrap();
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let (view, binding) = attach(&rt, &view, &folder);
    let stop = Arc::new(AtomicBool::new(false));
    let swapper = {
        let stop = Arc::clone(&stop);
        let folder = folder.clone();
        let outside = outside.clone();
        std::thread::spawn(move || {
            let (sub, parked, link) = (
                folder.join("sub"),
                folder.join("parked"),
                folder.join("link"),
            );
            symlink(&outside, &link).unwrap();
            while !stop.load(Ordering::Relaxed) {
                // sub -> parked, link -> sub, then restore.
                if fs::rename(&sub, &parked).is_ok() {
                    let _ = fs::rename(&link, &sub);
                    let _ = fs::rename(&sub, &link);
                    let _ = fs::rename(&parked, &sub);
                }
            }
        })
    };
    let c = ctx(&view);
    for i in 0..300 {
        let _ = rt.create_file(
            &c,
            &local(&binding, &["sub"]),
            &format!("n{i}.txt"),
            b"x",
            &op(),
        );
    }
    stop.store(true, Ordering::Relaxed);
    swapper.join().unwrap();
    assert_eq!(
        fs::read_dir(&outside).unwrap().count(),
        0,
        "file escaped the binding"
    );
}

#[test]
fn values_returned_to_the_frontend_never_contain_physical_paths() {
    let fixture = Fixture::new();
    let folder = fixture.folder("private-folder-name-xyz");
    fs::write(folder.join("a.txt"), b"a").unwrap();
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let selection = select(&rt, &view, &folder);
    let attached = rt.attach_directory(&ctx(&view), &selection, &op()).unwrap();
    let binding = attached.receipt.binding_id.clone();
    let c = ctx(&attached.workspace);
    let page = rt.list_entries(&c, &local(&binding, &[]), None).unwrap();
    let receipt = rt
        .create_file(&c, &local(&binding, &[]), "b.txt", b"b", &op())
        .unwrap();
    let root = fixture.temp.path().to_string_lossy().into_owned();
    for json in [
        serde_json::to_string(&selection).unwrap(),
        serde_json::to_string(&attached).unwrap(),
        serde_json::to_string(&page).unwrap(),
        serde_json::to_string(&receipt).unwrap(),
        serde_json::to_string(&rt.list_workspaces().unwrap()).unwrap(),
    ] {
        assert!(!json.contains(&root), "{json}");
        assert!(!json.contains('/'), "{json}");
    }
    // The leaf folder name is the only presentation label.
    assert!(
        attached
            .workspace
            .bindings
            .iter()
            .any(|b| b.label == "private-folder-name-xyz")
    );
}

#[test]
fn wire_values_use_camel_case_and_base64_bytes() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    fs::write(folder.join("a.txt"), b"hello").unwrap();
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let (view, binding) = attach(&rt, &view, &folder);
    let c = ctx(&view);
    let entry = rt
        .list_entries(&c, &local(&binding, &[]), None)
        .unwrap()
        .entries[0]
        .clone();
    let handle = rt
        .open_read(&c, &local(&binding, &["a.txt"]), &entry.file_identity)
        .unwrap();
    let page = rt.read_file(&c, &handle.read_handle_id, 0, 5).unwrap();
    let value = serde_json::to_value(&page).unwrap();
    assert_eq!(value["bytesBase64"], "aGVsbG8=");
    assert_eq!(value["eof"], true);
    assert!(value.get("contentGeneration").is_some());
    let caps = serde_json::to_value(rt.capabilities(true)).unwrap();
    assert_eq!(
        caps,
        serde_json::json!({"localResources":"available","nativeDirectoryPicker":"available","managedWorkspace":"available","multiWindow":false,"sidecar":false})
    );
    let error = serde_json::to_value(
        rt.list_entries(&c, &local(&binding, &[".."]), None)
            .unwrap_err(),
    )
    .unwrap();
    assert_eq!(
        error,
        serde_json::json!({"code":"invalid_locator","reason":"invalid_name"})
    );
    let outcome = serde_json::to_value(RuntimeWorkspaceOutcome::Pending).unwrap();
    assert_eq!(outcome, serde_json::json!({"state":"pending"}));
}

#[test]
fn creation_into_a_parent_moved_out_of_the_binding_leaves_no_orphan_file() {
    let fixture = Fixture::new();
    let outside = fixture.folder("outside");
    let folder = fixture.folder("folder");
    fs::create_dir(folder.join("sub")).unwrap();
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let (view, binding) = attach(&rt, &view, &folder);
    let stop = Arc::new(AtomicBool::new(false));
    let mover = {
        let stop = Arc::clone(&stop);
        let (inside, away) = (folder.join("sub"), outside.join("sub"));
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if fs::rename(&inside, &away).is_ok() {
                    std::thread::yield_now();
                    fs::rename(&away, &inside).unwrap();
                }
            }
        })
    };
    let c = ctx(&view);
    let mut created = Vec::new();
    for i in 0..400 {
        let name = format!("n{i}.txt");
        if rt
            .create_file(&c, &local(&binding, &["sub"]), &name, b"x", &op())
            .is_ok()
        {
            created.push(name);
        }
    }
    stop.store(true, Ordering::Relaxed);
    mover.join().unwrap();
    let mut on_disk: Vec<String> = fs::read_dir(folder.join("sub"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    on_disk.sort();
    created.sort();
    assert_eq!(on_disk, created, "a refused creation left its file behind");
}

// ----- independent review findings (2026-10-07) -----

fn break_registry_writes(fixture: &Fixture) {
    // A directory in place of the temporary file makes every save fail.
    fs::create_dir(fixture.state().join("registry.json.tmp")).unwrap();
}

fn restore_registry_writes(fixture: &Fixture) {
    fs::remove_dir(fixture.state().join("registry.json.tmp")).unwrap();
}

#[test]
fn failed_registry_write_changes_nothing_in_memory_or_after_restart() {
    let fixture = Fixture::new();
    let a = fixture.folder("a");
    let b = fixture.folder("b");
    let (view, binding) = {
        let rt = fixture.open();
        let view = new_workspace(&rt, "w");
        let (view, binding) = attach(&rt, &view, &a);

        break_registry_writes(&fixture);
        let detach_id = op();
        assert_eq!(
            code(rt.detach_directory(&ctx(&view), &binding, &detach_id)),
            RuntimeErrorCode::OutcomeUnknown
        );
        // The retry must not report a detach that never reached disk.
        assert_eq!(
            code(rt.detach_directory(&ctx(&view), &binding, &detach_id)),
            RuntimeErrorCode::OutcomeUnknown
        );
        let listed = rt.list_workspaces().unwrap();
        assert!(
            listed[0]
                .bindings
                .iter()
                .any(|item| item.binding_id == binding)
        );
        assert_eq!(
            code(rt.rename_workspace(&ctx(&view), "renamed", &op())),
            RuntimeErrorCode::OutcomeUnknown
        );
        assert_eq!(rt.list_workspaces().unwrap()[0].name, "w");
        let selection = select(&rt, &view, &b);
        let attach_id = op();
        assert_eq!(
            code(rt.attach_directory(&ctx(&view), &selection, &attach_id)),
            RuntimeErrorCode::OutcomeUnknown
        );
        assert_eq!(rt.list_workspaces().unwrap()[0].bindings.len(), 2);
        // Once writes work again the same attach completes exactly once.
        restore_registry_writes(&fixture);
        let attached = rt
            .attach_directory(&ctx(&view), &selection, &attach_id)
            .unwrap();
        assert_eq!(attached.workspace.bindings.len(), 3);
        (attached.workspace, binding)
    };
    let rt = fixture.open();
    let restored = rt.list_workspaces().unwrap();
    assert_eq!(restored, vec![view.clone()]);
    assert!(
        restored[0]
            .bindings
            .iter()
            .any(|item| item.binding_id == binding)
    );
}

#[test]
fn workspace_creation_operations_are_never_forgotten() {
    let fixture = Fixture::new();
    let rt = fixture.open();
    let id = op();
    let created = rt.create_workspace("w", &id).unwrap();
    let mut view = created.workspace.clone();
    // More operations than the retained log (1024 records) so eviction runs.
    for i in 0..1100 {
        view = rt
            .rename_workspace(&ctx(&view), &format!("n{i}"), &op())
            .unwrap();
    }
    match rt.recover_workspace(&id) {
        RuntimeWorkspaceOutcome::Ready { receipt } => assert_eq!(receipt, created.receipt),
        other => panic!("forgotten creation: {other:?}"),
    }
    assert_eq!(
        rt.create_workspace("w", &id).unwrap().receipt,
        created.receipt
    );
    assert_eq!(rt.list_workspaces().unwrap().len(), 1);
    assert_eq!(
        fs::read_dir(fixture.state().join("managed"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn a_fifo_with_a_waiting_writer_is_never_opened() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    let fifo_path = folder.join("pipe.txt");
    let c_path =
        std::ffi::CString::new(fifo_path.clone().into_os_string().into_encoded_bytes()).unwrap();
    // SAFETY: valid NUL-terminated path for a test FIFO.
    assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
    let released = Arc::new(AtomicBool::new(false));
    let writer = {
        let released = Arc::clone(&released);
        let path = fifo_path.clone();
        std::thread::spawn(move || {
            // Blocks until some reader opens the FIFO.
            let _file = fs::OpenOptions::new().write(true).open(path).unwrap();
            released.store(true, Ordering::SeqCst);
        })
    };
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let (view, binding) = attach(&rt, &view, &folder);
    let error = rt
        .open_read(&ctx(&view), &local(&binding, &["pipe.txt"]), "x")
        .unwrap_err();
    assert_eq!(error.code, RuntimeErrorCode::Denied);
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        !released.load(Ordering::SeqCst),
        "the broker opened the FIFO"
    );
    // Release the writer for cleanup.
    drop(fs::File::open(&fifo_path).unwrap());
    writer.join().unwrap();
}

#[test]
fn a_bound_folder_recreated_with_a_reused_inode_is_reported_replaced() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let (view, binding) = attach(&rt, &view, &folder);
    let mut replaced = false;
    for _ in 0..50 {
        fs::remove_dir(&folder).unwrap();
        fs::create_dir(&folder).unwrap();
        let error = rt.list_entries(&ctx(&view), &local(&binding, &[]), None);
        if let Err(error) = error {
            assert_eq!(error.reason, Some(RuntimeErrorReason::FolderReplaced));
            replaced = true;
            continue;
        }
        panic!("a recreated folder was accepted as the bound folder");
    }
    assert!(replaced);
}

#[test]
fn handles_of_an_outdated_context_do_not_hold_limit_slots() {
    let fixture = Fixture::new();
    let folder = fixture.folder("folder");
    let other = fixture.folder("other");
    for i in 0..5 {
        fs::write(folder.join(format!("{i}.txt")), b"x").unwrap();
    }
    let rt = fixture.open();
    let view = new_workspace(&rt, "w");
    let (view, binding) = attach(&rt, &view, &folder);
    let page = rt
        .list_entries(&ctx(&view), &local(&binding, &[]), None)
        .unwrap();
    for entry in &page.entries[..4] {
        rt.open_read(
            &ctx(&view),
            &local(&binding, &[&entry.name]),
            &entry.file_identity,
        )
        .unwrap();
    }
    // Attaching another folder moves the context; the old handles are dead.
    let (view, _) = attach(&rt, &view, &other);
    let fifth = &page.entries[4];
    rt.open_read(
        &ctx(&view),
        &local(&binding, &[&fifth.name]),
        &fifth.file_identity,
    )
    .unwrap();
}

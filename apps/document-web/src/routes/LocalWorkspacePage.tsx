import { useEffect, useMemo, useRef, useState, type FormEvent } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Dialog, Heading, Modal } from 'react-aria-components';
import { AppShell } from '../components/app-shell/AppShell';
import {
  decodePreview, formatBytes, localRuntimeKeys, runtimeMessage, useLocalEntries, useLocalWorkspaces, useRuntimeCapabilities, useRuntimeOperation,
  useUnresolvedRuntimeOperations,
} from '../application/local-workspace';
import { useRuntime } from '../runtime/runtime-context';
import {
  contextOf, isRuntimeFailure, MAX_READ_RANGE, type BindingSummary, type DirectorySelection, type LocalEntry, type LocalRef, type LocalWorkspace,
} from '../runtime/contract';
import dialogStyles from '../components/document/DocumentMetadataEditor.module.css';
import workspaceStyles from './DocumentWorkspace.module.css';
import styles from './LocalWorkspace.module.css';

const availabilityLabel = (value: 'available' | 'unavailable' | undefined) => value === 'available' ? '利用できます' : '利用できません';

function Problem({ error }: { error: unknown }) {
  return error ? <p className={styles.alert} role="alert">{runtimeMessage(error)}</p> : null;
}

export function LocalWorkspacePage() {
  const runtime = useRuntime();
  const client = useQueryClient();
  const capabilities = useRuntimeCapabilities();
  const desktop = capabilities.data?.localResources === 'available' && capabilities.data.managedWorkspace === 'available';
  // On the desktop the list also explains an unavailable runtime (another
  // instance, unreadable records, unsupported platform) with its typed reason.
  const workspaces = useLocalWorkspaces(desktop || (capabilities.isSuccess && runtime.kind === 'desktop'));
  const unresolved = useUnresolvedRuntimeOperations();
  const [selectedId, setSelectedId] = useState<string>();
  // An unresolved operation pins its Workspace so its confirmation stays reachable.
  const pinnedId = unresolved.find((item) => item.workspaceId)?.workspaceId;
  const selected = workspaces.data?.find((item) => item.workspaceId === (pinnedId ?? selectedId)) ?? workspaces.data?.[0];
  const [notice, setNotice] = useState('');
  const refresh = () => client.invalidateQueries({ queryKey: localRuntimeKeys.all });

  return (
    <AppShell activeNavigation="local-workspaces" headerContext="ローカルWorkspace" mainLabel="ローカルWorkspace" showContextPanel={false}>
      <div className={styles.page}>
        <header className={workspaceStyles.pageHeader}>
          <h1>ローカルWorkspace</h1>
          <p className={styles.note}>この端末だけで使う作業場所です。Workspace名とフォルダーの場所は連動しません。追加したフォルダーは他の担当者や端末へ共有されません。</p>
        </header>
        <dl className={styles.summary} aria-label="実行環境">
          <div><dt>実行環境</dt><dd>{runtime.kind === 'desktop' ? 'デスクトップ版' : 'ブラウザー版'}</dd></div>
          <div><dt>ローカルフォルダー</dt><dd>{availabilityLabel(capabilities.data?.localResources)}</dd></div>
          <div><dt>フォルダー選択</dt><dd>{availabilityLabel(capabilities.data?.nativeDirectoryPicker)}</dd></div>
          <div><dt>管理フォルダー</dt><dd>{availabilityLabel(capabilities.data?.managedWorkspace)}</dd></div>
        </dl>
        {capabilities.isPending && <p className={styles.note}>実行環境を確認しています…</p>}
        {capabilities.isSuccess && !desktop && (
          <p className={styles.note}>
            {runtime.kind === 'browser'
              ? 'このブラウザー版ではローカルフォルダーとWorkspaceを利用できません。デスクトップ版で開いてください。文書・タスク・検索はこのまま利用できます。'
              : 'この端末ではローカルフォルダーを利用できません。手順書の確認項目を参照してください。'}
          </p>
        )}
        {desktop && workspaces.isPending && <p className={styles.note}>ローカルWorkspaceを読み込んでいます…</p>}
        {workspaces.isError && <Problem error={workspaces.error} />}
        {desktop && unresolved.length > 0 && (
          <p className={styles.note}>結果を確認していない操作があります。「結果を確認」で確定するまで、他のWorkspaceやフォルダーへは移動できません。</p>
        )}
        {desktop && workspaces.data && (
          <div className={styles.layout}>
            <WorkspaceList workspaces={workspaces.data} selected={selected} onSelect={setSelectedId} blocked={unresolved.length > 0}
              onCreated={(workspace) => { setSelectedId(workspace.workspaceId); setNotice(`Workspace「${workspace.name}」を作成しました。`); }} />
            {selected
              ? <WorkspaceDetail key={selected.workspaceId} workspace={selected} canPick={capabilities.data?.nativeDirectoryPicker === 'available'} onNotice={setNotice} refresh={refresh} blocked={unresolved.length > 0} />
              : <section className={styles.panel}><p>Workspaceはまだありません。「新しいWorkspace」で作成してください。</p></section>}
          </div>
        )}
        <p className={styles.status} role="status" aria-live="polite">{notice}</p>
      </div>
    </AppShell>
  );
}

const CREATE_WORKSPACE = 'create-workspace';
const NO_PLACE = {};

function WorkspaceList({ workspaces, selected, onSelect, onCreated, blocked }: {
  workspaces: LocalWorkspace[]; selected?: LocalWorkspace; onSelect: (id: string) => void; onCreated: (workspace: LocalWorkspace) => void; blocked: boolean;
}) {
  const runtime = useRuntime();
  const client = useQueryClient();
  const unresolved = useUnresolvedRuntimeOperations();
  // Reopen the dialog of an uncertain creation after returning to the screen.
  const [open, setOpen] = useState(() => unresolved.some((item) => item.key === CREATE_WORKSPACE));
  const [name, setName] = useState('');
  const trigger = useRef<HTMLButtonElement>(null);
  const create = useRuntimeOperation(
    CREATE_WORKSPACE,
    NO_PLACE,
    (value: string, operationId: string) => runtime.workspace.createLocalWorkspace(value, operationId),
    async (created) => {
      await client.invalidateQueries({ queryKey: localRuntimeKeys.workspaces });
      onCreated(created.workspace);
      setOpen(false);
      setName('');
      requestAnimationFrame(() => trigger.current?.focus());
    },
  );
  const pending = create.state.status === 'pending';
  const unknown = create.state.status === 'unknown';
  const close = () => {
    if (pending || unknown) return;
    create.reset();
    setOpen(false);
    requestAnimationFrame(() => trigger.current?.focus());
  };
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (pending || (!unknown && !name.trim())) return;
    void create.submit(name);
  };
  return (
    <nav className={styles.panel} aria-labelledby="local-workspace-list-title">
      <h2 id="local-workspace-list-title">Workspace</h2>
      <ul className={styles.list} aria-label="ローカルWorkspace一覧">
        {workspaces.map((workspace) => (
          <li key={workspace.workspaceId}>
            <button type="button" className={styles.itemButton} aria-current={workspace.workspaceId === selected?.workspaceId ? 'true' : undefined}
              disabled={blocked && workspace.workspaceId !== selected?.workspaceId}
              onClick={() => onSelect(workspace.workspaceId)}>{workspace.name}</button>
          </li>
        ))}
      </ul>
      {unknown && !open
        ? <button ref={trigger} type="button" className={workspaceStyles.secondaryButton} onClick={() => setOpen(true)}>Workspace作成の結果を確認</button>
        : <button ref={trigger} type="button" className={workspaceStyles.secondaryButton} disabled={blocked && !open} onClick={() => setOpen(true)}>新しいWorkspace</button>}
      <Modal isOpen={open} onOpenChange={(value) => { if (!value) close(); }} isDismissable={!pending && !unknown} isKeyboardDismissDisabled={pending || unknown} className={`${dialogStyles.modal} ${styles.dialogModal}`}>
        <Dialog aria-labelledby="local-workspace-create-title" className={dialogStyles.dialog}>
          <form onSubmit={submit} className={styles.form}>
            <Heading id="local-workspace-create-title" slot="title">新しいWorkspace</Heading>
            <p className={styles.note}>この端末の管理フォルダーを自動で作ります。名前はいつでも変更でき、フォルダーの場所には使われません。</p>
            <label className={workspaceStyles.formField}>Workspace名
              <input aria-label="Workspace名" autoFocus maxLength={80} value={create.state.status === 'unknown' ? create.state.input : name}
                disabled={pending || unknown} onChange={(event) => setName(event.target.value)} />
            </label>
            {create.state.status === 'failed' && <Problem error={create.state.error} />}
            {unknown && <Problem error={create.state.status === 'unknown' ? create.state.error : undefined} />}
            <div className={dialogStyles.actions}>
              {unknown
                ? <button type="button" className={workspaceStyles.secondaryButton} onClick={() => { setOpen(false); requestAnimationFrame(() => trigger.current?.focus()); }}>あとで確認する</button>
                : <button type="button" className={workspaceStyles.secondaryButton} onClick={close} disabled={pending}>キャンセル</button>}
              <button type="submit" className={workspaceStyles.primaryButton} aria-disabled={pending || (!unknown && !name.trim())}>
                {unknown ? '結果を確認' : pending ? '作成中…' : '作成する'}
              </button>
            </div>
          </form>
        </Dialog>
      </Modal>
    </nav>
  );
}

// `nonce` changes on every 「開く」: the folder view lists its location again and
// drops alerts, preview and pages, but keeps the file name and content typed.
type Browse = { bindingId: string; locator: string[]; nonce?: number };

function WorkspaceDetail({ workspace, canPick, onNotice, refresh, blocked }: {
  workspace: LocalWorkspace; canPick: boolean; onNotice: (text: string) => void; refresh: () => Promise<void>; blocked: boolean;
}) {
  const runtime = useRuntime();
  const client = useQueryClient();
  const unresolved = useUnresolvedRuntimeOperations();
  const keys = useMemo(() => ({
    rename: `ws:${workspace.workspaceId}:rename`, attach: `ws:${workspace.workspaceId}:attach`, detach: `ws:${workspace.workspaceId}:detach`,
  }), [workspace.workspaceId]);
  const place = useMemo(() => ({ workspaceId: workspace.workspaceId }), [workspace.workspaceId]);
  const heading = useRef<HTMLHeadingElement>(null);
  const addButton = useRef<HTMLButtonElement>(null);
  // Returning to the screen restores the place of any unresolved operation.
  const [renaming, setRenaming] = useState(() => unresolved.some((item) => item.key === keys.rename));
  const [draftName, setDraftName] = useState(workspace.name);
  const renameButton = useRef<HTMLButtonElement>(null);
  const [picking, setPicking] = useState(false);
  const [folderProblem, setFolderProblem] = useState<unknown>();
  const unresolvedDetach = unresolved.find((item) => item.key === keys.detach)?.input as BindingSummary | undefined;
  const [detaching, setDetaching] = useState<BindingSummary | undefined>(() => unresolvedDetach);
  const detachTrigger = useRef<HTMLButtonElement | null>(null);
  const [browse, setBrowse] = useState<Browse | undefined>(() => unresolved.find((item) => item.workspaceId === workspace.workspaceId && item.browse)?.browse);

  const rename = useRuntimeOperation(
    keys.rename,
    place,
    (name: string, operationId: string) => runtime.workspace.renameWorkspace(contextOf(workspace), name, operationId),
    async (renamed) => {
      await client.invalidateQueries({ queryKey: localRuntimeKeys.workspaces });
      setRenaming(false);
      onNotice(`Workspace名を「${renamed.name}」に変更しました。フォルダーの場所は変わりません。`);
      requestAnimationFrame(() => renameButton.current?.focus());
    },
  );
  const attach = useRuntimeOperation(
    keys.attach,
    place,
    (selection: DirectorySelection, operationId: string) => runtime.resources.attachDirectory(contextOf(workspace), selection, operationId),
    async (attached) => {
      await client.invalidateQueries({ queryKey: localRuntimeKeys.workspaces });
      const label = attached.workspace.bindings.find((item) => item.bindingId === attached.receipt.bindingId)?.label ?? 'フォルダー';
      onNotice(`フォルダー「${label}」を追加しました。`);
    },
  );
  const detach = useRuntimeOperation(
    keys.detach,
    place,
    (binding: BindingSummary, operationId: string) => runtime.resources.detachDirectory(contextOf(workspace), binding.bindingId, operationId),
    async () => {
      const label = detaching?.label ?? 'フォルダー';
      if (browse?.bindingId === detaching?.bindingId) setBrowse(undefined);
      setDetaching(undefined);
      await client.invalidateQueries({ queryKey: localRuntimeKeys.workspaces });
      onNotice(`フォルダー「${label}」を解除しました。フォルダーの中身はそのままです。`);
      requestAnimationFrame(() => heading.current?.focus());
    },
  );

  async function addFolder() {
    if (picking || attach.state.status === 'pending') return;
    if (attach.state.status === 'unknown') { void attach.submit(attach.state.input); return; }
    setPicking(true);
    setFolderProblem(undefined);
    try {
      const selection = await runtime.dialog.chooseDirectory(contextOf(workspace));
      if (!selection) { onNotice('フォルダーの選択を取り消しました。変更はありません。'); return; }
      await attach.submit(selection);
    } catch (error) {
      setFolderProblem(error);
      if (isRuntimeFailure(error) && error.code === 'stale_context') await refresh();
    } finally {
      setPicking(false);
    }
  }

  function closeDetach() {
    if (detach.state.status === 'pending' || detach.state.status === 'unknown') return;
    detach.reset();
    setDetaching(undefined);
    requestAnimationFrame(() => detachTrigger.current?.focus());
  }

  const bindingFailure = attach.state.status === 'failed' ? attach.state.error : attach.state.status === 'unknown' ? attach.state.error : folderProblem;
  const browsed = workspace.bindings.find((item) => item.bindingId === browse?.bindingId);

  return (
    <section className={styles.panel} aria-labelledby="local-workspace-title">
      <div className={styles.toolbar}>
        <h2 id="local-workspace-title" ref={heading} tabIndex={-1}>{workspace.name}</h2>
        {!renaming && <button ref={renameButton} type="button" className={styles.inlineButton} disabled={blocked} onClick={() => { setDraftName(workspace.name); setRenaming(true); }}>名前を変更</button>}
      </div>
      {renaming && (
        <form className={styles.toolbar} onSubmit={(event) => { event.preventDefault(); if (rename.state.status !== 'pending' && draftName.trim()) void rename.submit(draftName); }}
          onKeyDown={(event) => { if (event.key === 'Escape' && rename.state.status !== 'pending' && rename.state.status !== 'unknown') { rename.reset(); setRenaming(false); requestAnimationFrame(() => renameButton.current?.focus()); } }}>
          <input aria-label="新しいWorkspace名" autoFocus maxLength={80} value={rename.state.status === 'unknown' ? rename.state.input : draftName}
            disabled={rename.state.status === 'unknown'} onChange={(event) => setDraftName(event.target.value)} />
          <button type="submit" className={workspaceStyles.secondaryButton}>{rename.state.status === 'unknown' ? '結果を確認' : '変更する'}</button>
          {rename.state.status === 'failed' && <Problem error={rename.state.error} />}
          {rename.state.status === 'unknown' && <Problem error={rename.state.error} />}
        </form>
      )}
      <h3>フォルダー</h3>
      <ul className={styles.list} aria-label={`${workspace.name}のフォルダー`}>
        {workspace.bindings.map((binding) => (
          <li key={binding.bindingId} className={styles.binding}>
            <span className={styles.bindingLabel}>{binding.label}</span>
            <span className={styles.source}>{binding.source === 'managed' ? '自動で作成した管理フォルダー' : '追加したフォルダー'}{binding.available ? '' : '・利用できません'}</span>
            <button type="button" className={workspaceStyles.secondaryButton} aria-label={`${binding.label}を開く`} disabled={blocked}
              onClick={() => setBrowse({ bindingId: binding.bindingId, locator: [], nonce: (browse?.nonce ?? 0) + 1 })}>開く</button>
            {binding.source === 'explicit' && (
              <button type="button" className={workspaceStyles.secondaryButton} aria-label={`${binding.label}を解除`}
                disabled={blocked && unresolvedDetach?.bindingId !== binding.bindingId}
                onClick={(event) => { detachTrigger.current = event.currentTarget; setDetaching(binding); }}>解除</button>
            )}
          </li>
        ))}
      </ul>
      {canPick
        ? <button ref={addButton} type="button" className={workspaceStyles.secondaryButton} aria-disabled={picking || attach.state.status === 'pending'}
          disabled={blocked && attach.state.status !== 'unknown' && attach.state.status !== 'pending'} onClick={() => void addFolder()}>
          {attach.state.status === 'unknown' ? '結果を確認' : picking ? 'フォルダーを選択中…' : 'フォルダーを追加'}
        </button>
        : <p className={styles.note}>この環境ではフォルダー選択画面を利用できません。</p>}
      <Problem error={bindingFailure} />
      <Modal isOpen={Boolean(detaching)} onOpenChange={(value) => { if (!value) closeDetach(); }} isDismissable={detach.state.status !== 'pending'} className={`${dialogStyles.modal} ${styles.dialogModal}`}>
        <Dialog aria-labelledby="local-detach-title" className={dialogStyles.dialog}>
          <Heading id="local-detach-title" slot="title">フォルダーの解除</Heading>
          <p>「{detaching?.label}」をこのWorkspaceから外します。フォルダーの中身は削除されません。</p>
          {detach.state.status === 'failed' && <Problem error={detach.state.error} />}
          {detach.state.status === 'unknown' && <Problem error={detach.state.error} />}
          <div className={dialogStyles.actions}>
            {detach.state.status === 'unknown'
              ? <button type="button" className={workspaceStyles.secondaryButton} onClick={() => { setDetaching(undefined); requestAnimationFrame(() => detachTrigger.current?.focus()); }}>あとで確認する</button>
              : <button type="button" className={workspaceStyles.secondaryButton} onClick={closeDetach} disabled={detach.state.status === 'pending'}>キャンセル</button>}
            <button type="button" className={workspaceStyles.primaryButton} aria-disabled={detach.state.status === 'pending'}
              onClick={() => { if (detaching && detach.state.status !== 'pending') void detach.submit(detach.state.status === 'unknown' ? detach.state.input : detaching); }}>
              {detach.state.status === 'unknown' ? '結果を確認' : '解除する'}
            </button>
          </div>
        </Dialog>
      </Modal>
      {browse && browsed && <FolderBrowser key={`${browse.bindingId}/${browse.locator.join('/')}`} workspace={workspace} binding={browsed} locator={browse.locator}
        refresh={browse.nonce ?? 0} onNavigate={(locator) => setBrowse({ bindingId: browse.bindingId, locator, nonce: browse.nonce })} onNotice={onNotice} blocked={blocked} />}
    </section>
  );
}

function FolderBrowser({ workspace, binding, locator, refresh, onNavigate, onNotice, blocked }: {
  workspace: LocalWorkspace; binding: BindingSummary; locator: string[]; refresh: number; onNavigate: (locator: string[]) => void; onNotice: (text: string) => void; blocked: boolean;
}) {
  const runtime = useRuntime();
  const client = useQueryClient();
  const ref: LocalRef = useMemo(() => ({ bindingId: binding.bindingId, locator }), [binding.bindingId, locator]);
  const place = useMemo(() => ({ workspaceId: workspace.workspaceId, browse: ref }), [workspace.workspaceId, ref]);
  const [cursors, setCursors] = useState<string[]>([]);
  const entries = useLocalEntries(workspace, ref, cursors.at(-1));
  const [stickyProblem, setStickyProblem] = useState<unknown>();
  const [preview, setPreview] = useState<{ name: string; size: number; truncated: boolean; text?: string; binary?: boolean }>();
  const [previewProblem, setPreviewProblem] = useState<unknown>();
  // A read belongs to the listing it started from: a re-list or a listing
  // failure starts a new generation, and a read that finishes later is dropped.
  const generation = useRef(0);
  const reading = useRef<number | null>(null);
  const [fileName, setFileName] = useState('');
  const [content, setContent] = useState('');
  const title = [binding.label, ...locator].join(' / ');

  useEffect(() => {
    const error = entries.error;
    if (!error) return;
    setStickyProblem(error);
    // Content previewed from a folder that can no longer be listed is not trustworthy.
    generation.current += 1;
    setPreview(undefined);
    setPreviewProblem(undefined);
    if (isRuntimeFailure(error) && (error.code === 'stale_context' || error.code === 'not_found' || error.reason === 'folder_replaced')) {
      void client.invalidateQueries({ queryKey: localRuntimeKeys.workspaces });
    }
  }, [client, entries.error]);

  // A listing that succeeds again ends an earlier failure, except a changed
  // context: its explanation stays until 「開く」 or another folder is opened,
  // as the automatic re-fetch with the new context would hide it at once.
  useEffect(() => {
    if (!entries.isSuccess || entries.isFetching || stickyProblem === undefined) return;
    if (isRuntimeFailure(stickyProblem) && stickyProblem.code === 'stale_context') return;
    setStickyProblem(undefined);
  }, [entries.isSuccess, entries.isFetching, stickyProblem]);

  const create = useRuntimeOperation(
    `ws:${workspace.workspaceId}:file:${binding.bindingId}:${locator.join('/')}`,
    place,
    (input: { name: string; content: string }, operationId: string) =>
      runtime.resources.createFile(contextOf(workspace), ref, input.name, new TextEncoder().encode(input.content), operationId),
    async (receipt) => {
      setFileName('');
      setContent('');
      await client.invalidateQueries({ queryKey: ['local-runtime', 'entries', workspace.workspaceId] });
      onNotice(`ファイル「${receipt.ref.locator.at(-1)}」を作成しました。`);
    },
  );

  // 「開く」 on the shown location lists it again from the first page, once.
  // Earlier alerts and the preview end with it; the create-form draft is kept.
  const shownRefresh = useRef(refresh);
  const { reset: resetCreate } = create;
  useEffect(() => {
    if (shownRefresh.current === refresh) return;
    shownRefresh.current = refresh;
    generation.current += 1;
    setStickyProblem(undefined);
    setPreview(undefined);
    setPreviewProblem(undefined);
    resetCreate();
    // From a later page, switching to the first page fetches it; refetching
    // the page being left would only send a wasted request.
    void client.invalidateQueries({ queryKey: ['local-runtime', 'entries', workspace.workspaceId], refetchType: cursors.length > 0 ? 'none' : 'active' });
    setCursors([]);
  }, [client, refresh, workspace.workspaceId, cursors.length, resetCreate]);

  async function open(entry: LocalEntry) {
    const started = generation.current;
    if (reading.current === started) return;
    reading.current = started;
    setPreviewProblem(undefined);
    const context = contextOf(workspace);
    try {
      const handle = await runtime.resources.openRead(context, { bindingId: binding.bindingId, locator: entry.locator }, entry.fileIdentity);
      try {
        const length = Math.min(handle.sizeBytes, MAX_READ_RANGE);
        const page = length > 0 ? await runtime.resources.readFile(context, handle, 0, length) : { bytes: new Uint8Array() };
        if (generation.current === started) {
          setPreview({ name: entry.name, size: handle.sizeBytes, truncated: handle.sizeBytes > length, ...decodePreview(page.bytes) });
        }
      } finally {
        await runtime.resources.closeRead(context, handle).catch(() => undefined);
      }
    } catch (error) {
      if (generation.current === started) {
        setPreview(undefined);
        setPreviewProblem(error);
      }
    } finally {
      if (reading.current === started) reading.current = null;
    }
  }

  const unknown = create.state.status === 'unknown';
  const pending = create.state.status === 'pending';
  const createInput = create.state.status === 'unknown' || create.state.status === 'pending' ? create.state.input : { name: fileName, content };
  const createProblem = create.state.status === 'failed' || create.state.status === 'unknown' ? create.state.error : undefined;
  const createSawChange = isRuntimeFailure(createProblem) && createProblem.code === 'stale_context';
  // A changed context is explained once: by the file form when its operation
  // hit it. The folder's own copy is dropped, so it does not reappear on retry.
  useEffect(() => {
    if (createSawChange) setStickyProblem((problem: unknown) => (isRuntimeFailure(problem) && problem.code === 'stale_context' ? undefined : problem));
  }, [createSawChange, stickyProblem]);
  const listingProblem = entries.isError ? entries.error : stickyProblem;
  const sameChange = createSawChange && isRuntimeFailure(listingProblem) && listingProblem.code === 'stale_context';

  return (
    <section className={styles.panel} aria-label={`${title} の閲覧`}>
      <div className={styles.toolbar}>
        <h3>{title}</h3>
        {locator.length > 0 && <button type="button" className={workspaceStyles.secondaryButton} disabled={blocked} onClick={() => onNavigate(locator.slice(0, -1))}>上の階層へ</button>}
      </div>
      {entries.isPending && <p className={styles.note}>内容を読み込んでいます…</p>}
      <Problem error={sameChange ? undefined : listingProblem} />
      {entries.data && !entries.isError && (
        <>
          {entries.data.entries.length === 0
            ? <p className={styles.note}>このフォルダーには表示できる項目がありません。</p>
            : (
              <table className={styles.entries} aria-label={`${title}の内容`}>
                <thead><tr><th scope="col">名前</th><th scope="col">種類</th><th scope="col">操作</th></tr></thead>
                <tbody>
                  {entries.data.entries.map((entry) => (
                    <tr key={entry.locator.join('/')}>
                      <td>{entry.kind === 'directory'
                        ? <button type="button" className={styles.inlineButton} disabled={blocked} onClick={() => { setStickyProblem(undefined); onNavigate(entry.locator); }}>{entry.name}</button>
                        : entry.name}</td>
                      <td>{entry.kind === 'directory' ? 'フォルダー' : 'ファイル'}</td>
                      <td>{entry.kind === 'file' && <button type="button" className={styles.inlineButton} aria-label={`${entry.name} の内容を表示`} onClick={() => void open(entry)}>内容を表示</button>}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          {entries.data.omittedCount > 0 && <p className={styles.note}>表示できない項目が{entries.data.omittedCount}件あります（リンク・特殊なファイル・使用できない名前）。</p>}
          <div className={styles.toolbar}>
            {cursors.length > 0 && <button type="button" className={workspaceStyles.secondaryButton} disabled={blocked} onClick={() => setCursors(cursors.slice(0, -1))}>前の100件</button>}
            {entries.data.nextCursor && <button type="button" className={workspaceStyles.secondaryButton} disabled={blocked} onClick={() => setCursors([...cursors, entries.data!.nextCursor!])}>次の100件</button>}
          </div>
        </>
      )}
      <Problem error={previewProblem} />
      {preview && (
        <section role="region" aria-label={`${preview.name} の内容`}>
          <p className={styles.note}>{formatBytes(preview.size)}{preview.truncated ? '（先頭1MiBのみ表示）' : ''}</p>
          {preview.binary ? <p className={styles.note}>テキストとして表示できないファイルです。</p> : <pre className={styles.preview}>{preview.text}</pre>}
        </section>
      )}
      <form className={styles.form} aria-label="この場所にファイルを作成" onSubmit={(event) => {
        event.preventDefault();
        if (pending || (!unknown && !fileName.trim())) return;
        void create.submit({ name: fileName.trim(), content });
      }}>
        <h4>この場所にファイルを作成</h4>
        <p className={styles.note}>既存のファイルは上書きしません。8MiBまでのテキストを保存できます。</p>
        <label className={workspaceStyles.formField}>ファイル名
          <input aria-label="ファイル名" maxLength={255} value={createInput.name} disabled={unknown || pending} onChange={(event) => setFileName(event.target.value)} />
        </label>
        <label className={workspaceStyles.formField}>内容
          <textarea aria-label="内容" rows={4} value={createInput.content} disabled={unknown || pending} onChange={(event) => setContent(event.target.value)} />
        </label>
        {create.state.status === 'failed' && <Problem error={create.state.error} />}
        {unknown && <Problem error={create.state.status === 'unknown' ? create.state.error : undefined} />}
        <div className={styles.toolbar}>
          <button type="submit" className={workspaceStyles.primaryButton} aria-disabled={pending}>{unknown ? '結果を確認' : pending ? '作成中…' : '作成する'}</button>
        </div>
      </form>
    </section>
  );
}

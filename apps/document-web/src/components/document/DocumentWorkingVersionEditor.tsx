import { useEffect, useRef, useState } from 'react';
import { skipToken, useQuery, useQueryClient } from '@tanstack/react-query';
import { Dialog, Heading, Modal } from 'react-aria-components';
import { documentApi, type DocumentDetail, type VersionDetail } from '../../application/document-workspace';
import { addedOriginalFileError, addedOriginalPathError, originalOf, prepareWorkingVersion, refreshWorkingQueries, replacementError, runWorkingOperation, workingOperationKey, workingEditorSource,
  type AddedWorkingOriginal, type EditManifest, type WorkingOperation } from '../../application/document-working-version';
import { createOperationId } from '../../application/operation-id';
import { mapApiProblem, problemFromUnknown } from '../../application/problem-mapping';
import { CapabilityButton } from '../shared/CapabilityButton';
import { ApiFeedback, LoadingState } from '../shared/ApiFeedback';
import styles from '../../routes/DocumentDetail.module.css';
import workspace from '../../routes/DocumentWorkspace.module.css';

type Props = {
  documentId: string; document?: DocumentDetail; version?: VersionDetail; purpose: 'published' | 'authoring';
  active: boolean; contextKey: string; showActions: boolean; onOpen: () => void; onClose: () => void;
  onModeChange?: (value: { contextKey: string; mode: 'create' | 'update' } | null) => void;
};
type EditorBaseline = {
  contextKey: string; generation: number; manifest: EditManifest; mode: 'create' | 'update';
  currentVersionId: string | null; displayVersionId: string; invalidated: boolean;
};
export function DocumentWorkingVersionEditor(props: Props) {
  const { documentId, document, version, active, contextKey, onOpen, onClose, showActions } = props;
  const client = useQueryClient();
  const key = workingOperationKey(documentId);
  const { data: operation } = useQuery<WorkingOperation | null>({ queryKey: key, queryFn: skipToken, enabled: false, initialData: null, gcTime: Infinity });
  const [rebase, setRebase] = useState<{ versionId: string; currentVersionId: string; revision: number } | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [baseline, setBaseline] = useState<EditorBaseline | null>(null);
  const [replaceBaseline, setReplaceBaseline] = useState(false);
  const baselineGeneration = useRef(0);
  const session = baseline?.contextKey === contextKey ? baseline : null;
  const refreshAttempt = useRef<object | null>(null);
  const refreshContext = useRef({ contextKey, generation: session?.generation });
  refreshContext.current = { contextKey, generation: session?.generation };
  useEffect(() => { refreshAttempt.current = null; setRefreshing(false); }, [contextKey]);
  const { sourceVersionId, mode, purpose } = workingEditorSource(document, version);
  const allowed = mode === 'update' ? version?.capabilities.edit?.status === 'available' : document?.capabilities.createVersion?.status === 'available';
  const canRebase = version?.capabilities.rebase?.status === 'available';
  const unresolved = operation?.status === 'unknown' || operation?.status === 'pending';
  const blocked = Boolean(operation && operation.status !== 'succeeded');
  const manifestQuery = useQuery({
    queryKey: ['document-edit-manifest', documentId, sourceVersionId, purpose],
    queryFn: () => documentApi.getVersionEditManifest(documentId, sourceVersionId!, purpose),
    enabled: Boolean(active && sourceVersionId && document && (mode === 'create' || version) && !operation), retry: false,
  });
  useEffect(() => { setRebase(null); }, [contextKey]);
  const wasActive = useRef(false);
  useEffect(() => {
    const entered = active && !wasActive.current; wasActive.current = active;
    if (entered && operation?.status === 'succeeded') client.setQueryData(key, null);
  }, [active, operation, client, key]);
  const manifest = manifestQuery.error ? undefined : manifestQuery.data;
  const manifestMatches = manifest && manifest.documentId === documentId && manifest.sourceVersionId === sourceVersionId && manifest.purpose === purpose;
  // The parent observer may notify after this explicit-refresh state update. Read the same Query snapshot, never stale props, for coherence.
  const documentRead = client.getQueryState<DocumentDetail>(['document', documentId, props.purpose]);
  const currentDocument = documentRead?.status === 'success' ? documentRead.data : undefined;
  const sourceContextMatches = Boolean(document && currentDocument
    && document.currentVersionId === currentDocument.currentVersionId
    && document.displayVersion.versionId === currentDocument.displayVersion.versionId
    && document.displayVersion.lifecycleState === currentDocument.displayVersion.lifecycleState
    && (!version || version.versionId !== currentDocument.displayVersion.versionId
      || version.lifecycleState.toUpperCase() === currentDocument.displayVersion.lifecycleState));
  const baselineMatches = Boolean(session && currentDocument && sourceContextMatches && manifestMatches && !manifestQuery.error
    && session.mode === mode && session.currentVersionId === currentDocument.currentVersionId
    && session.displayVersionId === currentDocument.displayVersion.versionId
    && session.manifest.documentRevision === currentDocument.revision
    && JSON.stringify(session.manifest) === JSON.stringify(manifest));
  useEffect(() => {
    if (!active) { setBaseline(null); setReplaceBaseline(false); return; }
    if (!operation && currentDocument && sourceContextMatches && manifestMatches && !manifestQuery.isFetching
      && (!session || (replaceBaseline && manifest.documentRevision === currentDocument.revision))) {
      setBaseline({ contextKey, generation: ++baselineGeneration.current, manifest, mode,
        currentVersionId: currentDocument.currentVersionId, displayVersionId: currentDocument.displayVersion.versionId, invalidated: false });
      setReplaceBaseline(false);
    } else if (session && !baselineMatches && !session.invalidated) {
      setBaseline({ ...session, invalidated: true });
    }
    // An explicit read gets one adoption attempt; a later background response cannot consume it.
    if (replaceBaseline) setReplaceBaseline(false);
  }, [active, operation, currentDocument, sourceContextMatches, manifestMatches, manifest, manifestQuery.isFetching, manifestQuery.error,
    session, replaceBaseline, baselineMatches, contextKey, mode]);
  useEffect(() => {
    props.onModeChange?.(active && session ? { contextKey, mode: session.mode } : null);
  }, [active, contextKey, session?.mode, props.onModeChange]);
  const problem = problemFromUnknown(operation?.error);
  const failure = problem ? mapApiProblem(problem).message : '応答を照合できません。';
  async function refresh() {
    const previousOperation = client.getQueryData<WorkingOperation | null>(key);
    if (refreshing || unresolved || previousOperation?.status === 'pending' || previousOperation?.status === 'unknown') return;
    const attempt = { contextKey, generation: session?.generation };
    refreshAttempt.current = attempt; setRefreshing(true);
    try {
      await refreshWorkingQueries(client, documentId);
      if (refreshAttempt.current !== attempt || refreshContext.current.contextKey !== attempt.contextKey
        || refreshContext.current.generation !== attempt.generation || client.getQueryData(key) !== previousOperation) return;
      client.setQueryData(key, null); setRebase(null); setReplaceBaseline(true);
    } catch { /* Retain the refusal until current state can be checked. */ }
    finally { if (refreshAttempt.current === attempt) { refreshAttempt.current = null; setRefreshing(false); } }
  }
  function start() { if (!blocked) { client.setQueryData(key, null); setBaseline(null); setReplaceBaseline(false); onOpen(); } }
  function confirmRebase() {
    if (!rebase || blocked || !canRebase || rebase.revision !== document?.revision || rebase.versionId !== version?.versionId || rebase.currentVersionId !== document.currentVersionId) return;
    void runWorkingOperation(client, { kind: 'rebase', documentId, sourceVersionId: rebase.versionId, currentVersionId: rebase.currentVersionId,
      body: { operationId: createOperationId(), expectedRevision: rebase.revision } });
    setRebase(null);
  }
  if (!active && !operation && !(showActions && (version?.capabilities.edit || version?.capabilities.rebase))) return null;
  return <section className={styles.contentSection} aria-label="作業版の編集">
    {showActions && <div className={styles.actionRow}>
      <CapabilityButton label="作業版を編集" availability={version?.capabilities.edit} disabled={blocked} onClick={start} />
      <CapabilityButton label="現行版へ基準を更新" availability={version?.capabilities.rebase} disabled={blocked || !document?.currentVersionId} onClick={() => {
        if (document?.currentVersionId && version) setRebase({ versionId: version.versionId, currentVersionId: document.currentVersionId, revision: document.revision });
      }} />
    </div>}
    {operation && <div aria-busy={operation.status === 'pending'}>
      {operation.status === 'pending' && <p role="status">保存結果を確認しています…</p>}
      {operation.status === 'unknown' && <section role="alert"><h2>保存結果を確認できません</h2>
        <p>同じ操作ID・対象版・全ファイルで再試行します。新しい要求へ変更せず、再読み込みやタブを閉じる前に結果を確認してください。</p><p>{failure}</p>
        <button type="button" onClick={() => void runWorkingOperation(client, operation.intent)}>同じ内容で再試行</button>
      </section>}
      {operation.status === 'rejected' && <section role="alert"><p>{failure} 最新状態を確認してから編集をやり直してください。</p>
        <button type="button" disabled={refreshing} onClick={() => void refresh()}>最新状態を確認</button></section>}
      {operation.status === 'succeeded' && (active || showActions) && <p role="status" className={styles.noticeSuccess}>{operation.message}</p>}
      {problem?.traceId && <small>照会ID: {problem.traceId}</small>}
    </div>}
    {active && (!operation || (operation.intent.kind !== 'rebase' && (operation.status === 'pending' || operation.status === 'rejected'))) && <>
      {manifestQuery.isPending && <LoadingState label="原本と補助ファイルを読み込み中" />}
      {manifestQuery.error && <ApiFeedback error={manifestQuery.error} onRetry={() => void manifestQuery.refetch()} />}
      {manifest && !manifestMatches && <p role="alert">編集元を照合できません。保存は送信していません。</p>}
      {session && <ManifestForm key={`${contextKey}:${session.generation}`} manifest={session.manifest} mode={session.mode} readOnly={Boolean(operation)}
        allowed={Boolean(allowed && baselineMatches && !session.invalidated)}
        onClose={onClose} onRefresh={refresh} refreshing={refreshing} />}
    </>}
    {active && ((!manifestMatches || operation?.status === 'unknown' || operation?.status === 'succeeded' || operation?.intent.kind === 'rebase')) && <button type="button" onClick={onClose}>版の一覧へ戻る</button>}
    <Modal isOpen={Boolean(rebase)} onOpenChange={value => { if (!value) setRebase(null); }} isDismissable className={styles.dialogScrim}>
      <Dialog aria-labelledby="working-rebase-title" className={styles.confirmDialog}>
        <Heading slot="title" id="working-rebase-title">作業版の基準更新を確認</Heading>
        <p>基準を現在の公開版へ更新します。原本・補助ファイルと版番号は変えず、公開も行いません。</p>
        <p>現行の公開版: {rebase?.currentVersionId}</p>
        <div className={styles.actionRow}><button type="button" autoFocus onClick={() => setRebase(null)}>キャンセル</button>
          <button type="button" disabled={!canRebase || blocked || rebase?.revision !== document?.revision || rebase?.currentVersionId !== document?.currentVersionId} onClick={confirmRebase}>基準更新を確定</button></div>
      </Dialog>
    </Modal>
  </section>;
}
function ManifestForm({ manifest, mode, allowed, readOnly, onClose, onRefresh, refreshing }: { manifest: EditManifest; mode: 'create' | 'update'; allowed: boolean; readOnly: boolean; onClose: () => void; onRefresh: () => Promise<void>; refreshing: boolean }) {
  const client = useQueryClient();
  const [title, setTitle] = useState(manifest.title);
  const [replacements, setReplacements] = useState<Map<string, File>>(new Map());
  const [itemIds, setItemIds] = useState(manifest.items.map(item => item.contentItemId));
  const [additions, setAdditions] = useState<AddedWorkingOriginal[]>([]);
  const [excluded, setExcluded] = useState<Map<string, number>>(new Map());
  const [adding, setAdding] = useState(false);
  const [newFile, setNewFile] = useState<File | null>(null);
  const [newPath, setNewPath] = useState('');
  const [preparing, setPreparing] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const controller = useRef<AbortController | null>(null);
  const currentAllowed = useRef(allowed); currentAllowed.current = allowed;
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; controller.current?.abort(); }; }, []);
  useEffect(() => { if (!allowed) controller.current?.abort(); }, [allowed]);
  const structuralChange = itemIds.length !== manifest.items.length || itemIds.some((id, index) => id !== manifest.items[index]?.contentItemId);
  const allItems = [...manifest.items, ...additions.map(addition => ({ contentItemId: addition.id, logicalPath: addition.logicalPath, ordinal: 0,
    representations: [{ representationId: addition.id, fileId: '', role: 'authoritative' as const, mediaType: addition.file.type,
      originalFilename: addition.file.name, sizeBytes: addition.file.size }] }))];
  const items = itemIds.map(id => allItems.find(item => item.contentItemId === id)!);
  const newError = addedOriginalPathError(newPath, items.map(item => item.logicalPath)) ?? (newFile ? addedOriginalFileError(newFile) : '追加原本ファイルを選択してください。');
  const controlsDisabled = preparing || readOnly || refreshing || !allowed;
  function move(id: string, delta: number) {
    if (controlsDisabled) return;
    setItemIds(previous => { const index = previous.indexOf(id); const next = [...previous];
      if (index < 0 || index + delta < 0 || index + delta >= next.length) return previous;
      [next[index], next[index + delta]] = [next[index + delta]!, next[index]!]; return next; });
  }
  function exclude(id: string) {
    if (controlsDisabled || itemIds.length <= 1) return;
    setExcluded(previous => new Map(previous).set(id, itemIds.indexOf(id))); setItemIds(previous => previous.filter(value => value !== id)); setError(null);
  }
  function restore(id: string) {
    if (controlsDisabled) return;
    const position = excluded.get(id)!;
    const item = allItems.find(value => value.contentItemId === id)!;
    if (additions.some(value => value.id === id || (itemIds.includes(value.id) && value.logicalPath === item.logicalPath))) {
      const invalid = addedOriginalPathError(item.logicalPath, items.map(value => value.logicalPath));
      if (invalid) { setError(new Error(invalid)); return; }
    }
    setItemIds(previous => { const next = [...previous]; next.splice(Math.min(position, next.length), 0, id); return next; });
    setExcluded(previous => { const next = new Map(previous); next.delete(id); return next; }); setError(null);
  }
  function confirmAddition() {
    if (controlsDisabled || !newFile || newError) return;
    const id = createOperationId(); setAdditions(previous => [...previous, { id, logicalPath: newPath.normalize('NFC'), file: newFile }]);
    setItemIds(previous => [...previous, id]); setAdding(false); setNewFile(null); setNewPath(''); setError(null);
  }
  const changed = structuralChange || title.trim().normalize('NFC') !== manifest.title.trim().normalize('NFC') || replacements.size > 0;
  const invalidReplacement = items.filter(item => !additions.some(addition => addition.id === item.contentItemId)).map(item => replacements.has(item.contentItemId) ? replacementError(originalOf(item), replacements.get(item.contentItemId)!) : null).find(Boolean);
  const submitAllowed = !readOnly && !refreshing && allowed && Boolean(title.trim()) && !invalidReplacement && !adding && itemIds.length > 0 && (mode === 'update' || changed);
  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (controller.current || !submitAllowed) return;
    const attempt = new AbortController(); controller.current = attempt; setPreparing(true); setError(null);
    try {
      const intent = await prepareWorkingVersion({ manifest, mode, title, replacements, signal: attempt.signal, ...(mode === 'update' ? { structure: { itemIds, additions: additions.filter(addition => itemIds.includes(addition.id)) } } : {}) });
      if (!mounted.current || attempt.signal.aborted || !currentAllowed.current) return;
      // No await between the final local guard and the synchronous pending cache write.
      void runWorkingOperation(client, intent);
    } catch (failure) { if (mounted.current && !attempt.signal.aborted) setError(failure); }
    finally { if (controller.current === attempt) { controller.current = null; if (mounted.current) setPreparing(false); } }
  }
  function cancelPreparation() { controller.current?.abort(); controller.current = null; setPreparing(false); }
  const problem = problemFromUnknown(error);
  const failure = problem ? mapApiProblem(problem).message : error instanceof Error ? error.message : 'ファイルを取得できませんでした。';
  return <form aria-label="作業版の原本を編集" className={styles.newVersionWorkspace} onSubmit={event => void submit(event)} aria-busy={preparing}>
    <label className={workspace.formField}>文書名<input value={title} disabled={preparing || readOnly || refreshing} required maxLength={500} onChange={event => { setTitle(event.target.value); setError(null); }} /></label>
    <p>{mode === 'update' ? '原本を追加・除外・上下移動できます。既存原本のパスと形式は変更できません。構成を変更した保存では表示順に0から再採番します。' : 'すべての原本を確認し、差し替える原本を選択してください。パス・順序・形式の変更、原本の追加・削除はできません。'}</p>
    {mode === 'update' && <>
      <p>保存する原本: {items.length}件・除外する原本: {excluded.size}件。最後の原本は除外できません。</p>
      {!adding ? <button type="button" disabled={controlsDisabled} onClick={() => setAdding(true)}>原本を追加</button> : <fieldset disabled={controlsDisabled}>
        <legend>追加する原本</legend>
        <label className={workspace.formField}>追加原本ファイル<input type="file" onChange={event => { setNewFile(event.target.files?.item(0) ?? null); setError(null); }} /></label>
        <label className={workspace.formField}>追加原本パス<input value={newPath} onChange={event => { setNewPath(event.target.value); setError(null); }} /></label>
        <p>相対パスを / 区切りで入力してください。追加時にNFCへ正規化します。</p>
        {newError && <p role="alert">{newError}</p>}
        <button type="button" disabled={Boolean(newError)} onClick={confirmAddition}>追加原本を確定</button>
        <button type="button" onClick={() => { setAdding(false); setNewFile(null); setNewPath(''); }}>追加を取消</button>
      </fieldset>}
    </>}
    <p>公開を確定するまで、現行の公開版は変わりません。保存時は保持する原本・補助ファイルも認可・監査を経て取得します。</p>
    <ul className={styles.fileList}>{items.map((item, index) => {
      const original = originalOf(item); const selected = replacements.get(item.contentItemId); const renditions = item.representations.filter(part => part.role === 'rendition'); const addition = additions.find(value => value.id === item.contentItemId); const ordinal = structuralChange ? index : item.ordinal;
      return <li key={item.contentItemId}><div>
        <h3>{original.originalFilename}</h3><dl><dt>固定パス</dt><dd>{item.logicalPath}</dd><dt>順序</dt><dd>{ordinal}</dd><dt>形式</dt><dd>{original.mediaType}</dd></dl>
        {!addition && <label className={workspace.formField}>差替ファイル: {original.originalFilename}（固定パス: {item.logicalPath}、順序: {ordinal}）<input type="file" disabled={preparing || readOnly || refreshing} onChange={event => {
          const file = event.target.files?.item(0); setReplacements(previous => { const next = new Map(previous); if (file) next.set(item.contentItemId, file); else next.delete(item.contentItemId); return next; }); setError(null);
        }} /></label>}
        {addition && <p>追加原本 · {addition.file.size.toLocaleString()} bytes（補助ファイルなし）</p>}
        {mode === 'update' && <div className={styles.actionRow}>
          <button type="button" disabled={controlsDisabled || index === 0} onClick={() => move(item.contentItemId, -1)}>{original.originalFilename}を上へ</button>
          <button type="button" disabled={controlsDisabled || index === items.length - 1} onClick={() => move(item.contentItemId, 1)}>{original.originalFilename}を下へ</button>
          <button type="button" disabled={controlsDisabled || items.length <= 1} onClick={() => exclude(item.contentItemId)}>{original.originalFilename}を除外</button>
        </div>}
        {selected ? <p>差替後: {selected.name} · {selected.size.toLocaleString()} bytes</p> : !addition && <p>原本を保持 · {original.sizeBytes.toLocaleString()} bytes</p>}
        <p>{selected ? `除外する補助ファイル: ${renditions.length}件` : `保持する補助ファイル: ${renditions.length}件`}</p>
        {renditions.length > 0 && <ul>{renditions.map(part => <li key={part.representationId}>{part.originalFilename}</li>)}</ul>}
      </div></li>;
    })}</ul>
    {excluded.size > 0 && <section aria-label="除外予定の原本"><h3>除外予定（保存まで取消可能）</h3><ul>{[...excluded.keys()].map(id => {
      const item = allItems.find(value => value.contentItemId === id)!; const original = originalOf(item);
      const renditionCount = item.representations.filter(part => part.role === 'rendition').length;
      return <li key={id}>{original.originalFilename} · {item.logicalPath} · 除外する補助ファイル: {renditionCount}件
        <button type="button" disabled={controlsDisabled} onClick={() => restore(id)}>{original.originalFilename}の除外を取消</button></li>;
    })}</ul></section>}
    <p>差し替え・除外対象の原本に属する補助ファイルを除外します。作業版から除外したファイルの永続的な履歴保存は保証されません。</p>
    <p>上限: 原本・補助ファイル計63件、各256 MiB、JSON 1 MiB、境界を含む送信全体1 GiB、送信120秒。</p>
    {!allowed && !readOnly && <div role="alert"><p>権限または文書の状態が更新されました。最新状態を確認してから編集をやり直してください。</p><button type="button" disabled={refreshing} onClick={() => { cancelPreparation(); void onRefresh(); }}>最新状態を確認</button></div>}
    {invalidReplacement && <p role="alert">{invalidReplacement}</p>}
    {Boolean(error) && <p role="alert">{failure} 保存は送信していません。</p>}
    <div className={styles.workflowFooter}>
      <button type="button" onClick={() => { cancelPreparation(); onClose(); }}>版の一覧へ戻る</button>
      {readOnly ? null : preparing ? <button type="button" onClick={cancelPreparation}>準備をキャンセル</button>
        : <button type="submit" className={workspace.primaryButton} disabled={!submitAllowed}>{mode === 'update' ? '作業版を保存' : '新しい作業版を作成'}</button>}
    </div>
  </form>;
}

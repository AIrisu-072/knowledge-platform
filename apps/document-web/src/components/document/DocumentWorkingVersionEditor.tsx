import { useEffect, useRef, useState } from 'react';
import { skipToken, useQuery, useQueryClient } from '@tanstack/react-query';
import { Dialog, Heading, Modal } from 'react-aria-components';
import { documentApi, type DocumentDetail, type VersionDetail } from '../../application/document-workspace';
import { originalOf, prepareWorkingVersion, refreshWorkingQueries, replacementError, runWorkingOperation, workingOperationKey, workingEditorSource,
  type EditManifest, type WorkingOperation } from '../../application/document-working-version';
import { createOperationId } from '../../application/operation-id';
import { mapApiProblem, problemFromUnknown } from '../../application/problem-mapping';
import { ApiFeedback, LoadingState } from '../shared/ApiFeedback';
import styles from '../../routes/DocumentDetail.module.css';
import workspace from '../../routes/DocumentWorkspace.module.css';

type Props = {
  documentId: string; document?: DocumentDetail; version?: VersionDetail; purpose: 'published' | 'authoring';
  active: boolean; contextKey: string; showActions: boolean; onOpen: () => void; onClose: () => void;
};
export function DocumentWorkingVersionEditor(props: Props) {
  const { documentId, document, version, active, contextKey, onOpen, onClose, showActions } = props;
  const client = useQueryClient();
  const key = workingOperationKey(documentId);
  const { data: operation } = useQuery<WorkingOperation | null>({ queryKey: key, queryFn: skipToken, enabled: false, initialData: null, gcTime: Infinity });
  const [rebase, setRebase] = useState<{ versionId: string; currentVersionId: string; revision: number } | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const { sourceVersionId, mode, purpose } = workingEditorSource(document, version);
  const allowed = mode === 'update' ? version?.capabilities.edit.status === 'available' : document?.capabilities.createVersion.status === 'available';
  const canRebase = version?.capabilities.rebase.status === 'available';
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
  const problem = problemFromUnknown(operation?.error);
  const failure = problem ? mapApiProblem(problem).message : '応答を照合できません。';
  async function refresh() {
    if (refreshing || unresolved) return;
    setRefreshing(true);
    try { await refreshWorkingQueries(client, documentId); client.setQueryData(key, null); setRebase(null); }
    catch { /* Retain the refusal until current state can be checked. */ }
    finally { setRefreshing(false); }
  }
  function start() { if (!blocked) { client.setQueryData(key, null); onOpen(); } }
  function confirmRebase() {
    if (!rebase || blocked || !canRebase || rebase.revision !== document?.revision || rebase.versionId !== version?.versionId || rebase.currentVersionId !== document.currentVersionId) return;
    void runWorkingOperation(client, { kind: 'rebase', documentId, sourceVersionId: rebase.versionId, currentVersionId: rebase.currentVersionId,
      body: { operationId: createOperationId(), expectedRevision: rebase.revision } });
    setRebase(null);
  }
  if (!active && !operation && !(showActions && (allowed && mode === 'update' || canRebase))) return null;
  return <section className={styles.contentSection} aria-label="作業版の編集">
    {showActions && <div className={styles.actionRow}>
      {mode === 'update' && allowed && <button type="button" disabled={blocked} onClick={start}>作業版を編集</button>}
      {canRebase && <button type="button" disabled={blocked || !document?.currentVersionId} onClick={() => {
        if (document?.currentVersionId && version) setRebase({ versionId: version.versionId, currentVersionId: document.currentVersionId, revision: document.revision });
      }}>現行版へ基準を更新</button>}
    </div>}
    {operation && <div aria-busy={operation.status === 'pending'}>
      {operation.status === 'pending' && <p role="status">保存結果を確認しています…</p>}
      {operation.status === 'unknown' && <section role="alert"><h2>保存結果を確認できません</h2>
        <p>同じ操作ID・対象版・全ファイルで再試行します。新しい要求へ変更せず、再読み込みやタブを閉じる前に結果を確認してください。</p><p>{failure}</p>
        <button type="button" onClick={() => void runWorkingOperation(client, operation.intent)}>同じ内容で再試行</button>
      </section>}
      {operation.status === 'rejected' && <section role="alert"><p>{failure} 最新状態を確認してから編集をやり直してください。</p>
        <button type="button" disabled={refreshing} onClick={() => void refresh()}>最新状態を確認</button></section>}
      {operation.status === 'succeeded' && <p role="status" className={styles.noticeSuccess}>{operation.message}</p>}
      {problem?.traceId && <small>照会ID: {problem.traceId}</small>}
    </div>}
    {active && (!operation || (operation.intent.kind !== 'rebase' && (operation.status === 'pending' || operation.status === 'rejected'))) && <>
      {manifestQuery.isPending && <LoadingState label="原本と補助ファイルを読み込み中" />}
      {manifestQuery.error && <ApiFeedback error={manifestQuery.error} onRetry={() => void manifestQuery.refetch()} />}
      {manifest && !manifestMatches && <p role="alert">編集元を照合できません。保存は送信していません。</p>}
      {manifestMatches && <ManifestForm key={`${contextKey}:${manifest.sourceVersionId}:${manifest.documentRevision}`} manifest={manifest} mode={mode} readOnly={Boolean(operation)}
        allowed={Boolean(allowed && document && manifest.documentRevision === document.revision && sourceVersionId === manifest.sourceVersionId)}
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
  const [preparing, setPreparing] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const controller = useRef<AbortController | null>(null);
  const currentAllowed = useRef(allowed); currentAllowed.current = allowed;
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; controller.current?.abort(); }; }, []);
  useEffect(() => { if (!allowed) controller.current?.abort(); }, [allowed]);
  const changed = title.trim().normalize('NFC') !== manifest.title.trim().normalize('NFC') || replacements.size > 0;
  const invalidReplacement = manifest.items.map(item => replacements.has(item.contentItemId) ? replacementError(originalOf(item), replacements.get(item.contentItemId)!) : null).find(Boolean);
  const submitAllowed = !readOnly && !refreshing && allowed && Boolean(title.trim()) && !invalidReplacement && (mode === 'update' || changed);
  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (controller.current || !submitAllowed) return;
    const attempt = new AbortController(); controller.current = attempt; setPreparing(true); setError(null);
    try {
      const intent = await prepareWorkingVersion({ manifest, mode, title, replacements, signal: attempt.signal });
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
    <label className={workspace.formField}>文書名<input value={title} disabled={preparing || readOnly} required maxLength={500} onChange={event => { setTitle(event.target.value); setError(null); }} /></label>
    <p>すべての原本を確認し、差し替える原本を選択してください。パス・順序・形式の変更、原本の追加・削除はできません。</p>
    <p>公開を確定するまで、現行の公開版は変わりません。保存時は保持する原本・補助ファイルも認可・監査を経て取得します。</p>
    <ul className={styles.fileList}>{manifest.items.map(item => {
      const original = originalOf(item); const selected = replacements.get(item.contentItemId); const renditions = item.representations.filter(part => part.role === 'rendition');
      return <li key={item.contentItemId}><div>
        <h3>{original.originalFilename}</h3><dl><dt>固定パス</dt><dd>{item.logicalPath}</dd><dt>順序</dt><dd>{item.ordinal}</dd><dt>形式</dt><dd>{original.mediaType}</dd></dl>
        <label className={workspace.formField}>差替ファイル: {original.originalFilename}（固定パス: {item.logicalPath}、順序: {item.ordinal}）<input type="file" disabled={preparing || readOnly} onChange={event => {
          const file = event.target.files?.item(0); setReplacements(previous => { const next = new Map(previous); if (file) next.set(item.contentItemId, file); else next.delete(item.contentItemId); return next; }); setError(null);
        }} /></label>
        {selected ? <p>差替後: {selected.name} · {selected.size.toLocaleString()} bytes</p> : <p>原本を保持 · {original.sizeBytes.toLocaleString()} bytes</p>}
        <p>{selected ? `除外する補助ファイル: ${renditions.length}件` : `保持する補助ファイル: ${renditions.length}件`}</p>
        {renditions.length > 0 && <ul>{renditions.map(part => <li key={part.representationId}>{part.originalFilename}</li>)}</ul>}
      </div></li>;
    })}</ul>
    <p>差し替える原本に属する補助ファイルだけを除外します。作業版から除外したファイルの永続的な履歴保存は保証されません。</p>
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

import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Dialog, Heading, Modal } from 'react-aria-components';
import { documentApi, type FolderDetail } from '../../application/document-workspace';
import { canCreateRootFolder, folderName, rootFolderOperations, rootFolderValidation, sendRootFolderOperation } from '../../application/document-root-folder';
import { metadataReason } from '../../application/document-metadata';
import { createOperationId } from '../../application/operation-id';
import { mapApiProblem, problemFromUnknown } from '../../application/problem-mapping';
import { CapabilityButton, availabilityReason } from '../shared/CapabilityButton';
import styles from './DocumentMetadataEditor.module.css';
import workspace from '../../routes/DocumentWorkspace.module.css';

const title = 'System Rootにフォルダーを作成';
export function RootFolderCreate({ root, readReady, reload, contextKey }: {
  root?: FolderDetail; readReady: boolean; reload: () => Promise<FolderDetail>; contextKey: string;
}) {
  const client = useQueryClient();
  const store = rootFolderOperations(client);
  const operation = useSyncExternalStore(store.subscribe, store.get);
  const [open, setOpen] = useState(false);
  const [name, setName] = useState('');
  const [reason, setReason] = useState('');
  const [reading, setReading] = useState(false);
  const [localError, setLocalError] = useState('');
  const opening = useRef(0);
  const refreshing = useRef(false);
  const triggerContainer = useRef<HTMLSpanElement>(null);
  useEffect(() => { opening.current += 1; refreshing.current = false; setReading(false); setOpen(false); }, [contextKey]);
  useEffect(() => () => { opening.current += 1; }, []);
  const pending = operation?.status === 'pending';
  const unknown = operation?.status === 'unknown';
  const allowed = readReady && canCreateRootFolder(root);
  const locked = Boolean(operation) || reading;
  const validation = rootFolderValidation(name, reason);
  const problem = problemFromUnknown(operation?.error);

  function show() {
    opening.current += 1; refreshing.current = false; setReading(false);
    setName(''); setReason(''); setLocalError(''); setOpen(true);
  }
  function close() {
    if (store.get()?.status === 'pending') return;
    opening.current += 1; refreshing.current = false; setReading(false); setOpen(false);
    requestAnimationFrame(() => triggerContainer.current?.querySelector('button')?.focus());
  }
  async function invalidate() {
    const parentId = store.get()?.request.parentFolderId;
    await Promise.all([
      client.invalidateQueries({ queryKey: ['folder-tree', 'root'] }),
      client.invalidateQueries({ queryKey: ['folder-tree', parentId] }),
    ]);
  }
  async function submit(event?: React.FormEvent) {
    event?.preventDefault();
    const saved = store.get();
    if (saved) {
      if (saved.status === 'unknown') await sendRootFolderOperation({ store, request: saved.request, send: documentApi.createFolder, invalidate });
      return;
    }
    if (!allowed || validation || refreshing.current) return;
    refreshing.current = true; setReading(true); setLocalError('');
    const generation = opening.current;
    try {
      // refetch deliberately bypasses the 15s query freshness window before each new request.
      const current = await reload();
      if (generation !== opening.current || store.get()) return;
      if (!canCreateRootFolder(current)) { setLocalError('現在のSystem Rootでは作成できません。権限と最新の状態を確認してください。'); return; }
      await sendRootFolderOperation({ store, request: {
        operationId: createOperationId(), folderId: createOperationId(), parentFolderId: current.folderId,
        expectedParentRevision: current.revision, name: folderName(name), reason: metadataReason(reason),
      }, send: documentApi.createFolder, invalidate });
    } catch {
      if (generation === opening.current) setLocalError('最新のSystem Rootを取得できません。状態を読み直してから作成してください。');
    } finally {
      if (generation === opening.current) { refreshing.current = false; setReading(false); }
    }
  }
  async function review() {
    const saved = store.get();
    if (saved?.status !== 'rejected' || refreshing.current) return;
    const generation = opening.current;
    refreshing.current = true; setReading(true); setLocalError('');
    try {
      const current = await reload();
      if (generation !== opening.current || store.get() !== saved) return;
      if (!canCreateRootFolder(current)) { setLocalError('現在のSystem Rootでは作成できません。権限と最新の状態を確認してください。'); return; }
      if (store.clearSettled(saved)) { setName(saved.request.name); setReason(saved.request.reason); }
    } catch {
      if (generation === opening.current) setLocalError('最新のSystem Rootを取得できません。状態を読み直してから作成してください。');
    } finally {
      if (generation === opening.current) { refreshing.current = false; setReading(false); }
    }
  }
  return <>
    <span ref={triggerContainer}>{operation
      ? <button type="button" onClick={show}>{title}</button>
      : <CapabilityButton label={title} availability={root?.capabilities?.createFolder} disabled={!allowed} onClick={show} />}</span>
    <Modal isOpen={open} onOpenChange={value => { if (!value) close(); }} isDismissable={!pending} isKeyboardDismissDisabled={pending} className={styles.modal}>
      <Dialog aria-labelledby="root-folder-create-title" className={styles.dialog}>
        <Heading slot="title" id="root-folder-create-title">{title}</Heading>
        <p>登録先：System Root直下（選択中のフォルダーには作成しません）</p>
        <form onSubmit={submit} aria-busy={pending || reading}>
          <label className={workspace.formField}>フォルダー名<textarea aria-label="フォルダー名" rows={1} autoFocus value={operation?.request.name ?? name} disabled={locked || !allowed} onChange={event => { if (!locked) setName(event.target.value); }} /></label>
          <label className={workspace.formField}>作成理由<textarea aria-label="作成理由" rows={2} value={operation?.request.reason ?? reason} disabled={locked || !allowed} onChange={event => { if (!locked) setReason(event.target.value); }} /></label>
          {!operation && (name || reason) && validation && <p role="alert">{validation}</p>}
          {!operation && !allowed && <p role="alert">{root?.capabilities?.createFolder?.status === 'disabled' ? availabilityReason(root.capabilities.createFolder.reason) : 'System Rootの現在の状態を確認してください。'}</p>}
          {localError && <p role="alert">{localError}</p>}
          {pending && <p role="status">作成結果を確認しています…</p>}
          {reading && !operation && <p role="status">System Rootの最新の状態を確認しています…</p>}
          {unknown && <section role="alert"><h3>作成結果を確認できません</h3><p>同じ操作ID・同じ内容で再試行して結果を確認してください。新しい作成は開始できません。</p><p>要求はこのアプリのメモリー内だけに保持されます。ページ再読み込みやタブ終了で失われるため、このページからの離脱を避け、解決しない場合は操作IDを添えて管理者へ結果を確認してください。</p></section>}
          {operation && <p>操作ID：{operation.request.operationId}<br />作成先フォルダーID：{operation.request.parentFolderId}<br />新しいフォルダーID：{operation.request.folderId}</p>}
          {operation?.status === 'rejected' && <p role="alert">作成は拒否されました。最新の状態を取得し、名前と状態を見直してください。</p>}
          {problem && <p role={unknown ? undefined : 'alert'}>{unknown ? `再試行の結果：${problem.code}。初回の作成結果は未確定です。同じ要求を保持して管理者へ確認してください。` : problem.code === 'REVISION_CONFLICT' ? '同名のフォルダーがあるか、フォルダーの状態が更新されています。名前と最新の状態を見直してください。' : mapApiProblem(problem).message}</p>}
          {operation?.status === 'succeeded' && <p role="status">フォルダーを作成しました。</p>}
          <div className={styles.actions}>
            <button type="button" disabled={pending} onClick={close}>{operation && !pending ? '閉じる' : 'キャンセル'}</button>
            {unknown ? <button type="button" onClick={() => void submit()}>同じ内容で再試行</button>
              : operation?.status === 'rejected' ? <button type="button" disabled={reading} onClick={() => void review()}>最新の状態を取得して見直す</button>
                : operation?.status === 'succeeded' ? <button type="button" onClick={() => { if (store.clearSettled(operation)) close(); }}>確認して閉じる</button>
                  : <button className={workspace.primaryButton} type="submit" disabled={pending || reading || !allowed || Boolean(validation)}>作成する</button>}
          </div>
        </form>
      </Dialog>
    </Modal>
  </>;
}

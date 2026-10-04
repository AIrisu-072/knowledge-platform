import { recordsMatchTask } from './evidence-workspace';
import { useQuery } from '@tanstack/react-query';
import { workApi, WorkApiError, type WorkSession, type TaskSummary } from './work-workspace';
export const evidenceRecordsKey = (session: WorkSession, task: TaskSummary) => ['organization', session.principalId, session.actingAssignmentId, 'evidence-context', task.id, task.attemptId];
/** Current authorization is checked on each read; immutable records are not an authorization cache. */
export function useEvidenceRecords(session: WorkSession, task: TaskSummary, enabled = true) {
  return useQuery({ queryKey: [...evidenceRecordsKey(session, task), task.revision], enabled, staleTime: 0, gcTime: 0, retry: false, queryFn: async () => {
    const [evidence, findings] = await Promise.all([workApi.listEvidence(task.id), workApi.listFindings(task.id)]);
    const decisions = (await Promise.all(findings.items.map((finding) => workApi.listDecisions(finding.id)))).flatMap((page) => page.items);
    const records = { evidence: evidence.items, findings: findings.items, decisions };
    const snapshot = !recordsMatchTask(task, records) && task.handoffSnapshotId ? await workApi.getSnapshot(task.handoffSnapshotId) : undefined;
    if (!recordsMatchTask(task, records, snapshot)) throw new WorkApiError(409, 'RECORD_SCOPE_CHANGED');
    return records;
  } });
}

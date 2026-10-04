# Organization Client v0 — Phase 2 Domain / API / Authorization Design

Status: WRITTEN / INDEPENDENT PHASE2 REVIEW PENDING. Product implementation,
Phase3 concrete source and Phase4 runtime qualification have not started.

## 1. Fixed inputs and interpretation

Read the [frozen Phase1](2026-10-02-organization-client-v0-product-ux-design.md),
its [authority record](2026-10-02-organization-client-v0-product-ux-approval.md),
the [original requirement map](../execution/organization-client-v0-requirements-map.md)
and existing normative `spec/` contracts. Source remains accepted PR43 H2.
The owner's §50 permits this faithful formalization; §21 explicitly delegates
evaluation of the smallest non-destructive attempt model. This design supplies
concrete contracts for the approved product, not a new approval/workflow product.

No legal/credit eligibility rules, organization-specific approval thresholds,
production identity, live LLM/provider connection, business scoring, external
system integration or unqualified Audit/Search implementation is selected.
Synthetic definitions demonstrate the contract without encoding real customers.

## 2. Authority and implementation boundaries

| Owner | Authoritative records | Explicitly not owned |
|---|---|---|
| Work authority | WorkContext, WorkItem/Attempt, workflow definitions/instances, assignment, artifacts and submitted membership, Evidence/Finding/HumanDecision, structured AgentExecution | Document content/lifecycle/ACL, external provider state, physical local paths |
| Organization policy authority | OrganizationalUnit, BusinessRole, RoleAssignment, Delegation, responsibility/resource/work-view policy | Authentication credentials, directory display-name authority, provider ACL overrides |
| Document Platform | Existing documents/files, Versions, issued Revisions, OCC, metadata, policies, DSI/Diff and access audit | WorkItem completion/return or Agent decisions |
| Local runtime | Principal/device-local managed roots, directory handles/bindings, scoped local operations | Shared artifact authority or business assignment/permission |
| Search Platform | Existing derived retrieval/discovery projections | Final authorization, business mutation, credentials/physical paths |
| Audit infrastructure | Separate durable accountability pipeline after its own qualification | Work history SSOT, KPI/analytics, personal memory or chat bodies |

Minimum implementation units: `work-domain` (values/invariants),
`work-application` (commands/queries/ports), `work-repository-postgres` (owned
transactions/schema), `work-api-http` (transport) and an Organization composition
root. Do not introduce infrastructure into Domain/Application or SQL into HTTP.
The root reuses existing Document router/services/composition building blocks;
if a small factory extraction is necessary, the old `document-server` profiles,
commands and acceptance remain behaviorally identical. No second Document logic.

Use the selected PostgreSQL stack with a logically owned `work` schema and
explicit separate migration ledger; do not reuse Document or Audit migration
numbers/checksums. Same physical database is allowed in synthetic composition,
not required by the domain. Work records and required Work event staging share
one transaction. Provider publication and Work submit are not a distributed
transaction. No automatic production migrations or server-start migrations.

The existing Architecture Contract's Document-only metadata inventory remains
Document-specific. Before product code, add the separately owned Work boundary
and dependency rules in a narrow, reviewable spec change consistent with this
approved expansion; do not silently make Document own Work tables.

## 3. Identifiers and immutable definition versions

New persistent IDs and caller operation IDs are UUIDv7. Reuse existing wire
conventions: RFC3339 UTC instants, nonnegative int64 OCC revisions, explicit
nullable/union fields, opaque cursors, JSON Schema2020-12/OpenAPI3.2.1 and RFC9457.
Display labels and external locators are never database identity. A public ID is
not a grant. Display identity resolution remains fail-soft and non-authoritative.

WorkflowDefinition version contains WorkStepDefinition[], TransitionDefinition[]
and ResponsibilitySegment[]. Once referenced by a WorkflowInstance the version
is immutable; new definitions receive a new version. WorkStep identifies WorkType,
responsibility segment, expected artifact/result schema and WorkViewProfile.
Transition identifies source/target steps and action kind. Segment identifies
the BusinessRole/responsibility required over its member steps. v0 synthetic
fixtures use explicit forward/return paths; no general BPMN interpreter, script
condition language, ad-hoc parallel join or invented business approval rules.

WorkflowInstance binds one definition version and WorkContext. WorkContext has
kind `case|routine_run|batch|request`, title, context-shared reference membership
and OCC revision. Its permitted metadata is definition/schema-bound, not an
unlimited arbitrary JSON sink. A customer's name is synthetic fixture data only.
WorkType provides the human task kind and schema/profile reference; it is not a
separate office domain. Both projections query these same records.

## 4. Minimal WorkItem and rework model

Alternatives evaluated under original§21:

| Model | Benefit | Cost / disposition |
|---|---|---|
| Stable WorkItem + immutable completed WorkAttempt | Stable task identity for routes/links; attempt-scoped drafts/assignment; simple explicit causal history | Selected; one current attempt pointer, monotonic attempt number and OCC required |
| New WorkItem per activation | Every execution has its own primary identity | Duplicates logical task identity and requires extra grouping to recover the same continuity; unnecessary for v0 |
| Reopen/overwrite completed execution | Superficially small | Rejected; destroys the expressly required past submission/history |

`WorkItem { id, contextId, workflowInstanceId, stepId, workTypeId,
currentAttemptId, revision }` is stable. `WorkAttempt { id, workItemId,
attemptNumber, state, assignmentId?, dueAt?, previousSubmissionId?,
returnInstructionId?, createdAt, completedAt? }` owns one execution.
Unique `(workItemId, attemptNumber)` and one current attempt are enforced in the
same repository transaction. State is `ready|active|held|completed`; these are
work execution states, never Document lifecycle. `returned` is a derived
attention/rework label. No `draft`, `overdue`, `waiting_for_confirmation` or
`agent_active` lifecycle variants are added.

Claim/assignment changes ready→active. Hold active→held; resume held→active,
subject to current responsibility and definition-permitted action. Completion
and submit close an active attempt. They never reopen completed attempts.
Complete is allowed only for a definition step whose outcome needs no forward
handoff. Submit requires the defined forward transition and an immutable
submission snapshot. No UI infers action availability from these strings.

Return targets a prior submitted WorkItem/Attempt in the same workflow causal
path. It records an immutable ReturnInstruction with previousSubmissionId,
bounded reason, returning actor/responsibility, targetWorkItemId and causal
currentAttemptId; it closes the returning active attempt and creates the new
target attempt with the next attemptNumber. Earlier completed attempts and
snapshots stay byte-for-byte unchanged. The new attempt starts ready with its
own private draft; role-based assignment is re-evaluated. It may refer to prior
submission but never edits it. Subsequent forward submit creates a fresh next
step attempt, not reopening its previously closed one.

Only definition-permitted transitions are executable. Reject return to a task
outside the workflow, missing prior submission, stale current attempt, already
completed source, disallowed target, or any concurrent winning transition.
Defining new branch/join/cancellation/compensation semantics is out of v0 scope.

## 5. Role, assignment and delegation policy

`OrganizationalUnit` supplies candidate pool/default WorkViewProfile/available
BusinessRole/default ResourcePolicy references. `BusinessRole` identifies an
owned policy revision and responsibility. `RoleAssignment` binds actual
PrincipalRef(provider, principalId) to role and optional unit, validFrom,
validUntil (exclusive, nullable = no scheduled end), and revokedAt. Clock is
trusted server UTC; reject inverted ranges. Formal assignments are not inferred
from department membership.

`Delegation` links a delegating active RoleAssignment to a recipient principal,
bounded role/action/resource scope, validity interval, reason and revocation.
It cannot widen the original role/assignment scope or outlive its effective
authority; nested delegation is not supported in v0. Revocation/expiration of
either source or delegation ends derived eligibility immediately on the next
authorization check. Retain historical attribution; do not rewrite prior acts.

`WorkAssignment` binds one attempt, principal, acting RoleAssignment or
Delegation reference and responsibility segment. It is distinct from role
eligibility and from the WorkItem's stable identity. At most one current active
assignment per attempt in v0. Reassignment atomically ends the old assignment,
records actor/reason, creates the new assignment and increments relevant OCC;
the old assignee loses future private access. It does not erase their historical
contribution or fabricate provider access for the new assignee.

Minimal policy actions are separately named: `context.read`, `context.progress.read`,
`context.history.read`, `queue.read`, `work.read`,
`work.claim`, `work.assign`, `work.edit`, `work.submit`, `work.return`,
`work.complete`, `work.hold`, `work.resume`, `evidence.register`,
`finding.register`, `decision.record`, `agent.request`, `organization.manage`.
Definitions/policies grant only necessary actions to synthetic roles. A role
label, department, task name or client-supplied actingAssignmentId never grants
an action. Only a current authorized management responsibility can configure
roles/assignments/delegations; no generic default administrator or allow-all.

## 6. Authorization evaluation and disclosure

Application takes `VerifiedActor` from the trusted identity adapter, never from
body/query/headers chosen by the frontend. A supplied `actingAssignmentId` or
delegation selection identifies requested responsibility; server resolves and
checks that it belongs to the verified actor and remains valid on every action.
It is not a credential or alternate principal. The effective-context revision
binds current policy/assignment/delegation/work state, not a reusable access grant.

For a protected action:

1. Authenticate and validate the current actor context.
2. Resolve requested active responsibility and current eligibility/action scope.
3. Check current attempt/assignment and workflow action/state under OCC.
4. Check artifact visibility and contextual membership.
5. Resolve provider references through the provider's current authorization.
6. Immediately before mutation commit or data/result disclosure, recheck relevant
   local revisions/current authority and required provider freshness. Unknown
   provider authorization is unavailable/deny, never an allow.

Use conditional repository operations/locking to prevent assignment expiry,
reassignment or policy revocation races from committing stale authority. Time is
re-evaluated at commit, including held transactions crossing validUntil. Remote
provider authorization cannot be made globally atomic with Work DB; record its
observed version/check time and perform fresh disclosure checks. Do not claim
instantaneous cross-provider revocation or use a stale snapshot as an ACL.

Minimal queue projection for `queue.read` contains opaque workItemId/attemptId,
generic WorkType/step label, permitted attention and claim availability. It
contains no private draft/title, customer/context label, filenames, Evidence,
notes or assignment-sensitive detail absent `work.read`. Counts, grouping,
filtering and cursors use the same authorized projection; they cannot reveal
hidden private fields through count/filter side channels. Eligibility alone
does not imply queue.read. Full task detail requires assignment/responsibility
and current policy, independently of queue eligibility.

Context continuity is separately authorized. `context.read` returns only context
ID, permitted display title/type and explicitly context-shared references.
`context.progress.read` returns a closed projection of step/WorkType labels,
ready/active/held/completed progress, permitted due/attention, own next-task link
and authorized submission/return existence. It never includes draft text,
filenames, private Evidence, candidate/decision bodies or other assignees' private
identity. `context.history.read` returns only permitted transition kind/time,
step and non-content causal references; reason, submission membership and actor
identity need their separate current read grants. Unknown/inaccessible references
are non-disclosing. Context filters/counts/cursors operate only on these allowed
fields and bind the specific context-read policy revisions. Sales responsibility
can retain context progress across downstream assignments without gaining their
private work. Neither context grant confers work.read or provider access.

`work_item_private` is scoped to current attempt assignment plus explicitly
authorized current responsibility, never all eligible candidates or downstream
roles. `handoff` applies to immutable submitted membership for its permitted
workflow recipients. `context_shared` is explicit sharing within context policy,
not all-organization public. Every content, preview, Evidence fragment, Agent
context and known-ID read applies the same rules. A submitted snapshot grants
no ongoing provider ACL; references can return unavailable/denied later.

Use hidden404 with resource-specific stable codes for unreadable known resources;
use403 only where existence is already legitimately disclosed and a particular
action is denied. List access can fail503/401 rather than returning an invented
empty list. Server-issued capability reasons distinguish permission, state,
assignment, stale context, provider unavailable and unsupported capability.
Capabilities are hints; mutation/disclosure re-evaluation is mandatory.

## 7. Working artifacts, submission and provider promotion

`WorkingArtifact` owns attempt ID, author, schema-bound editable value or an
opaque Work-owned content generation, visibility `work_item_private`, revision
and timestamps. New private draft bodies are served only through Work-authorized
endpoints. A raw Document/provider reference cannot be relabeled private:
already-shared sources remain `InputResourceRef` and retain their provider's
actual visibility. A private provider adapter could be added only after proving
matching attempt-scoped direct-read/list/history/preview isolation and current
reassignment/return revocation; no such external adapter is qualified in v0.

For bounded opaque draft bytes, the Work authority owns separate metadata and
storage namespace behind `WorkArtifactStorage`, reusing the existing selected
FileStorage adapter through a narrow interface. Each completed write has an
immutable generation ID, size and hash; editable draft metadata points to the
current generation. Replaced generations and submitted membership are never
mutated in place. No DocumentVersion, DocumentRevision, document ACL, publication,
parser/DSI/Diff or search authority is copied into this artifact store. Content
is downloaded as an untrusted attachment, not executed/rendered as active HTML.

The storage root is never independently static-served or exposed to native/Agent
paths. Every content/list/history/preview read checks current Work assignment,
visibility and provider authorization at the Work provider boundary. Reassignment
retains artifact history but removes the old assignee's future access; return
creates a new attempt-private draft and does not overwrite prior handoff bytes.
This applies through known IDs, cached references and range/recovery endpoints,
not merely the Work screen. Immutable storage staging and orphan reconciliation
reuse the existing adapter pattern; no destructive cleanup is introduced here.

`HandoffSnapshot` is append-only: id, workflow/context, sourceWorkItem/attempt,
submission number, actor/responsibility, createdAt, immutable artifact membership
and revision/digest references, selected Finding/HumanDecision/Evidence IDs,
target step/responsibility and causal predecessor. It never embeds mutable live
draft pointers as a completed submission. Each referenced Work record is pinned
to its immutable revision/version; current content reads remain authorized.

Before submit, every artifact must have an immutable shared-authority reference
with provider/resource/generation identity, logical locator and a verified
availability receipt. The qualified Work artifact provider is shared/server-owned
across devices but keeps draft disclosure `work_item_private` until the atomic
Work submission changes the authorized submitted membership. Shared storage does
not mean pre-submit shared visibility. Receipt validation happens inside the
server, not from frontend flags. Missing bytes, local-only IDs, mutable-only
versions, pending/failed/unknown uploads or forged receipts reject submit.

A selected local file is explicitly uploaded into the Work-owned private artifact
boundary before handoff; no automatic directory sync. Metadata and immutable bytes
are verified before it can be included. The submit transaction then exposes its
pinned generation as handoff to currently authorized recipients. This is the
minimal Shared Authoritative Provider permitted by the owner's §30 and must earn
its own storage/authorization/restart qualification; it is not assumed qualified
by the existing Document tests.

Document promotion remains a separate explicit action through the existing API,
client/BinaryTransportBridge and its existing authorization, operation IDs and
publication semantics. It may copy already-submitted authorized content into a
Document or reference an already-shared input, but it is not the mechanism for
keeping a new Work draft private. v0 never publishes a private draft into a
broader readable Document before submit and claims the Work label protects it.
Direct Document reads continue to obey Document's unchanged policies. Other
providers require separately qualified immutable identity/current authorization
and privacy before use as generated handoff content. Recipient access is always
rechecked and no Work command copies or silently expands provider ACLs.

Submit transaction locks current WorkItem/attempt/assignment, compares expected
revision and operation digest, freezes membership, closes source attempt,
records workflow history/required event staging, and creates exactly one ready
next attempt. No partial next-task visibility before commit. Private Work upload completed
before this transaction may remain unused after a failed submit; it stays private
and recoverable, never an automatically shared artifact or destructive rollback
of a separate Document operation.

## 8. Evidence / Finding / HumanDecision schema

`EvidenceRecord` fields:

- id, contextId, workItemId?, attemptId?, creator principal/responsibility,
  origin `human|search|document|agent|mcp|external|business_system`
- `sourceRef { providerId, resourceId, versionId?, revisionId? }` and
  provider-defined opaque `authoritativeLocator`; no physical path/credential
- `relevantLocation` and optional policy-permitted bounded fragment; fragment
  omission reason is explicit (`not_retained|unavailable|unsupported`)
- retrievedAt, recordedAt, provider/check provenance, originExecutionId?,
  coverage, uncertainty[], conflictReferences[] and visibility

Retain source-native coverage details losslessly. Common envelope class is
`complete|partial|none|unknown`; it never upgrades Document Full/Partial/None or
Unknown verdict, and keeps the original verdict/profile/locator alongside it.
Evidence from Search is registered by references/provenance, not an unconditional
copy of a Search response. Provider NO_RETENTION/no-copy policy can require
reference-only or execution-ephemeral evidence; unavailable retained proof stays
explicit. A pointer alone is not claimed to preserve a prohibited fragment.

`Finding { id, contextId, attemptId?, author, claim, evidenceIds[],
originExecutionId?, uncertainty[], conflicts[], visibility, createdAt }` is an
immutable submitted candidate; changed claim/support creates a new revision with
supersedes reference. At least one Evidence reference is required for an
evidence-backed Finding; a bare Agent suggestion remains SuggestedAction/Result,
never silently promoted to verified Finding.

`HumanDecision { id, findingId, findingRevision, decision:accepted|modified|
rejected, adoptedClaim?, reason?, evidenceRevisionRefs[], humanPrincipal,
actingResponsibility, attemptId, createdAt, supersedesDecisionId? }` is append-only.
Modified requires adoptedClaim; rejected never changes original Finding. The
verified invocation must be HumanInteractive for decision.record; Agent requests
cannot impersonate that Human. A new decision can supersede an earlier one while
preserving both. Decision is authority about the Human's chosen judgment, not an
override of source truth or permission to execute the suggested workflow action.

All references must belong to authorized context/attempt scope; reject cross-
context/private-reference probing. Collection, direct, comparison and Agent-result
reads recheck current visibility and provider policy. Store provenance and policy
disposition rather than unrestricted source/chat bodies. UI always differentiates
verified source facts, candidate claims and the Human's decision.

## 9. AgentExecution and context safety

`AgentExecution { id, contextId, workItemId, attemptId, requestedBy,
requesterResponsibility, executedBy, executorInvocationKind,
providerPrincipalBindings[], effectiveContextRevision, status, startedAt, endedAt? }`.
`requestedBy` is the verified requesting Human; `executedBy` is the verified
actual Agent executor. Each provider binding records the independently verified
provider principal used by that adapter; these identities are never assumed
identical or substituted for one another.
Status is `queued|running|succeeded|failed|cancelled|outcome_unknown`; it is
execution state, not WorkAttempt state. Result carries bounded summary plus
Finding/Evidence/GeneratedArtifact/SuggestedAction references and explicit
partial/uncertain/provider errors. HumanDecision references link later Human acts,
never a model-authored decision impersonating them.

`BuildAgentContext(actor, task, selectedResources) -> AuthorizedContext` returns
only current authorized task/context/responsibility/workspace/resources/tools/
history/evidence. Selection narrows access, never expands it. Revalidate before
each provider/tool operation and before result disclosure; task/role changes,
reassignment or revocation invalidate the execution context for further work.
Late responses are fenced by execution ID and context revision. Cancellation
does not imply remote side effects were undone; unknown outcome remains explicit.

v0 Agent integration is read/investigate/compare plus generated draft/candidate
artifacts in the execution's private scope. Workflow actions and HumanDecision
remain separate authorized commands initiated by a Human; no autonomous approval,
claim/reassign/submit/return or Document mutation tool is introduced. This is the
minimal collaboration path using the existing read-only Document MCP, not a
separate Agent business API. Any later autonomous action authority needs the
relevant explicit semantics/permissions and review.

An application-owned `AgentDispatchContext` binds execution ID, requester
principal and current responsibility, actual executor principal/invocation,
allowlisted read tools, selected resources and effective-context revision. It is
bounded execution state inside trusted application/adapter boundaries, not a
client-supplied principal or bearer credential. `agent.request` authorizes only
that scoped assistance; it creates no persistent OAuth/token/role grant. Every
tool/resource invocation and result disclosure requires the intersection of:
requester's current Work/context/source rights; executor's current execution
scope; and the actual provider principal's current authorization. Losing any
side denies further disclosure. Recheck before calling and before exposing output;
no broad service read may reveal data the requester cannot read.

The inherited Document MCP verifies provider `poc`, principal `poc-agent` and
Agent invocation at startup. Preserve that exact existing preflight. The new
application-owned dispatch/provider adapter must additionally verify its fixed
provider binding before each use and result disclosure; do not attribute this
new per-use obligation to the unchanged MCP implementation. Organization
synthetic `agent-01` is not renamed or equated to `poc-agent`. The synthetic
dispatch adapter explicitly records `executedBy=organization-synthetic/agent-01`
and the separately verified Document provider binding `poc/poc-agent`; only the
read-only allowlist is eligible. Requester-side Document authorization uses the
requester's actual trusted synthetic principal with the existing Document
Application authorization service, via a narrow provider-authorizer port. It
never substitutes the fixed `poc-human` profile or accepts identity fields from a
tool argument. Synthetic Document policies must separately grant the relevant
requester and provider principal where intended, and negative fixtures ensure
each can be denied independently. No business authorization evaluator is copied
into the Agent wrapper.

Existing Document MCP and its two original server profiles remain unchanged.
If a future provider cannot evaluate the current requester as well as its own
executor identity, that provider is unavailable for delegated Agent disclosure;
a wider executor grant is not a workaround. Actual MCP assertions remain
separate from simulated Agent-executor tests. Audit/business provenance retains
requester, acting responsibility, actual executor and actual provider identity.

GeneratedArtifact is a private candidate until saved into Work/Document through
the normal authorized artifact/promotion path. SuggestedAction carries a typed
proposed action and target plus supporting references; it is not executable
authority and is revalidated when a Human chooses it. Transcript is session-local
interaction in v0 and clears on identity/responsibility boundary change; important
results are server business records. No personal-memory system or Audit transcript
copy is introduced. Synthetic deterministic executor fixtures may test contracts;
they are labeled simulated, never a live-LLM study or MCP runtime proof.

## 10. Attention and WorkViewProfile projections

WorkViewProfile retains all fields from Phase1. Profile choice combines current
unit defaults with authorized role override; it cannot change data permissions.
Context and queue query modes return different projections of the same IDs.

Attention inputs are authoritative assignment/submission/return timestamps,
explicit dueAt, pending permitted confirmation and currently unavailable
prerequisites. `newly_assigned` uses principal×assignment seen state;
`returned` uses unresolved ReturnInstruction on current attempt; `overdue` means
now>dueAt for unfinished work. `due_soon` is present only when an explicit
WorkType/profile policy provides a lead duration; no invented universal threshold.
`waiting_for_confirmation` and `blocked` identify actual recorded/requested
confirmation or missing prerequisite, not inferred employee performance. Attention
may coexist; acknowledging attention does not complete the task. No analytics
or personnel-ranking purpose is introduced.

## 11. Workspace and ResourceBinding contract

`Workspace { id, name, revision, scope, createdBy }` is logical. EffectiveWorkspace
for a task merges its managed root reference, explicit bindings and current
policy-derived bindings. ResourceBinding fields: opaque id, workspaceId,
source `managed|explicit|policy_derived`, providerId/kind, logical resourceRef,
scope `principal_device_local|shared`, enabled, provenance and capability hints.
`accessible` is evaluated current-state, not an editable grant flag. Local path,
credential and invocation secrets stay below the provider/runtime boundary.
Server-visible local readiness/receipts are advisory observations only, never
proof granting filesystem access. Actual local authority stays in the broker;
shared submission rejects local references regardless of any reported receipt.

Current identity, active assignments/delegation, unit, context and task derive
policy bindings at query/use time. Cache keys include all authority revisions;
reassignment/expiry invalidates stale projections. Explicitly enabling a binding
affects ordinary exploration only and does not make inaccessible data accessible.
Detaching removes the binding, not the directory or provider content; no delete
file operation is part of v0. Rename Workspace changes only its logical name.

Creation is a bounded two-stage recoverable operation: server creates logical
Workspace/operation identity; capable runtime creates a private managed root
and optional chosen directory bindings; the client reports ready only after its broker confirms
the required runtime result. Server storage of that report is an advisory hint,
not cryptographic proof of a native host or authorization to access files. Lost response recovers by the same operation ID,
not another root. A partial runtime failure remains `runtime_unavailable` and
can retry without deleting arbitrary content. Browser cannot fabricate a managed
root. Its UI reports missing LocalResource capability and can use an existing
effective shared Workspace; the browser FS adapter is future/low priority.

Task resolution normally selects its effective Workspace automatically. The one
manual create dialog has name and optional local folders. Folder attachment never
converts the folder into a Workspace, couples names/paths or implies shared access.

## 12. Bounded Runtime Contract

Frontend application layer consumes these interfaces; presentation imports none
of the Tauri APIs. Types are source-independent contracts, not credentials:

```ts
type RuntimeCapabilities = {
  localResources: 'available' | 'unavailable';
  nativeDirectoryPicker: 'available' | 'unavailable';
  managedWorkspace: 'available' | 'unavailable';
  multiWindow: false; sidecar: false;
};
type ContextRef = { effectiveContextRevision: string; workspaceId: string };
type LocalRef = { bindingId: string; locator: string[] };
type RuntimeWorkspaceReceipt = { operationId: string; workspaceId: string; managedBindingId: string; runtimeRevision: string };
type RuntimeWorkspaceOutcome = { state: 'ready'; receipt: RuntimeWorkspaceReceipt } | { state: 'pending' | 'not_found' | 'unavailable' | 'outcome_unknown' };
type DirectorySelection = { selectionId: string };
type BindingReceipt = { operationId: string; bindingId: string; runtimeRevision: string };
type EntryPage = { entries: Array<{ locator: string[]; name: string; kind: 'file' | 'directory'; fileIdentity: string }>; nextCursor: string | null };
type ReadHandle = { readHandleId: string; contentGeneration: string; sizeBytes: number };
type BytePage = { bytes: Uint8Array; offset: number; contentGeneration: string; eof: boolean };
type FileReceipt = { operationId: string; ref: LocalRef; fileIdentity: string; sizeBytes: number; sha256: string };
interface WorkspaceRuntime {
  createWorkspace(context: ContextRef, operationId: string): Promise<RuntimeWorkspaceReceipt>;
  recoverWorkspace(operationId: string): Promise<RuntimeWorkspaceOutcome>;
}
interface NativeDialogCapability {
  chooseDirectory(context: ContextRef): Promise<DirectorySelection | null>;
}
interface LocalResourceCapability {
  attachDirectory(context: ContextRef, selection: DirectorySelection, operationId: string): Promise<BindingReceipt>;
  detachDirectory(context: ContextRef, bindingId: string, operationId: string): Promise<void>;
  listEntries(context: ContextRef, ref: LocalRef, cursor?: string): Promise<EntryPage>;
  openRead(context: ContextRef, ref: LocalRef, expectedFileIdentity: string): Promise<ReadHandle>;
  readFile(context: ContextRef, handle: ReadHandle, offset: number, length: number): Promise<BytePage>;
  closeRead(context: ContextRef, handle: ReadHandle): Promise<void>;
  createFile(context: ContextRef, parent: LocalRef, name: string, bytes: Uint8Array, operationId: string): Promise<FileReceipt>;
}
```

DirectorySelection is an opaque, single-use native picker receipt bound to this
window/principal/device/context; it contains no path in JavaScript. The broker
owns actual OS handles and protected local registry. Bindings are never valid
for another principal/device/window or after detach/context invalidation.
ContextRef is checked against trusted current session; client-supplied revision
cannot self-authorize. Offline cached grants do not authorize fresh operations.

Contract bounds: entry page≤100, read range≤1MiB, create payload≤8MiB, pending
native operations≤4, relative depth≤32 and each name≤255 UTF-8 bytes. These are
resource-safety ceilings, not user performance SLOs; Phase4 must prove them in
the actual broker. Larger writes/streaming/overwrite are outside v0. createFile
is exclusive/no-overwrite; exact retry returns its stored receipt only when
operation digest and resulting identity match. Uncertain completion is not a
new-path retry. listEntries returns an opaque file identity. openRead rechecks it and creates a
bounded per-context read handle tied to one content generation, not only an inode
or path. A read handle owns a bounded immutable private snapshot (maximum8MiB)
made with a verified stable-copy protocol: compare source file identity and
content generation before/after copy; concurrent mutation, including same-inode
write, truncate or replacement, rejects rather than returning a mixed snapshot.
If the OS cannot establish a stable generation/copy under concurrent writers,
openRead is unavailable/conflict until safe capture is possible; mtime/size alone
is insufficient. The snapshot is transient and remains principal/device-local.
Every range requires the same handle/generation/context; offset+length bounds are
checked against its fixed size. closeRead, detach, authority invalidation, expiry
or process exit invalidates handles; subsequent reads reject. At most4 handles
and32MiB snapshot bytes are retained, with a5-minute maximum lease, without
extending underlying authorization. Restart never resurrects an old read handle.
The same-inode negative test and actual safe-capture method are mandatory Phase4
proofs; unsupported capture is not a silent best-effort read.

Reject absolute/drive/UNC/device paths, empty/dot/dotdot components, embedded
separators/NUL, decoded traversal, reserved Windows device names and alternate
data streams. No URI/encoding second interpretation. Enforce confinement using
handle-relative no-follow operations and identity checks, not string-prefix
canonicalization alone. Reject symlinks/junctions/reparse-point escape and races
between check/open, including create parent replacement. If the selected Tauri/
OS libraries cannot prove these semantics, Phase4 is STOP, not an unsafe fallback.
No general filesystem/shell/executable command or request privilege flag.

Runtime outcomes use typed safe codes: unavailable, invalid_locator, denied,
stale_context, not_found, conflict, limit, cancelled, outcome_unknown. They
contain no physical paths or raw OS diagnostics. Tauri command permissions are
scoped to the app's trusted single main window/origin; remote content obtains
no broker commands. CSP/origin/API transport and native permission configuration
must be qualified without expanding the existing server's CORS/security rules.

## 13. HTTP / OpenAPI surface

Extend the common API conventions with a dedicated Organization path namespace
and one generated typed client boundary. This is not Human-versus-Agent APIs.
Document paths/types/client remain unchanged. Existing actor establishment and
Problem normalization are reused; DTOs/validation come from schemas, not hand-
copied frontend business models. Names below are the concrete v0 design surface.

All collection GETs use default50/max100 keyset pages and opaque cursors bound
to principal, acting responsibility, authorization revision, view/filter/sort.
No automatic all-pages traversal. Mutations use `operationId`,
`expectedRevision` and validated `actingAssignmentId` where responsibility is
required. Schema-specific input is closed (`additionalProperties:false`).

| Method / path under `/v1/organization` | operationId | Meaning / principal input |
|---|---|---|
| GET `/session` | getOrganizationSession | Verified actual principal and eligible current responsibility summaries; does not alter Document session |
| GET `/effective-context` | getEffectiveContext | taskId?, actingAssignmentId?; current authorization/profile/workspace/capabilities; selection is revalidated |
| GET `/units` | listOrganizationalUnits | Authorized unit summaries |
| GET `/roles` | listBusinessRoles | Authorized role/policy summaries |
| GET `/role-assignments` | listRoleAssignments | Own/current or management-authorized assignments |
| POST `/role-assignments` | createRoleAssignment | Management-authorized principal/role/unit/time scope and reason |
| POST `/role-assignments/{id}/revoke` | revokeRoleAssignment | Current management authority; no historical rewrite |
| GET `/delegations` | listDelegations | Authorized current/history delegation records |
| POST `/delegations` | createDelegation | Existing assignment, recipient, bounded scope/time/reason |
| POST `/delegations/{id}/revoke` | revokeDelegation | Current authorized revocation |
| GET `/tasks` | listWorkItems | view=context|queue, WorkType/context filters; authorized minimal/full projection only |
| GET `/tasks/{id}` | getWorkItem | Current attempt, authorized work, responsibilities and action hints |
| POST `/tasks/{id}/claim` | claimWorkItem | Claim ready current attempt with eligible responsibility; OCC prevents competing winners |
| POST `/tasks/{id}/assignment` | assignWorkItem | Assign/reassign to eligible principal/role with current work.assign and reason |
| POST `/tasks/{id}/actions` | executeWorkflowAction | Closed union complete|hold|resume; definition action ID and expected attempt |
| POST `/tasks/{id}/submit` | submitWorkItem | Frozen artifact/decision/evidence revision refs and forward transition |
| POST `/tasks/{id}/return` | returnWorkItem | Prior submission/target definition transition and bounded reason |
| GET `/tasks/{id}/attention` | getWorkAttention | Derived attention with source fact references; not lifecycle |
| POST `/tasks/{id}/attention-seen` | markWorkAttentionSeen | Own assignment attention acknowledgment only |
| GET `/work-contexts` | listWorkContexts | Authorized context projection |
| GET `/work-contexts/{id}` | getWorkContext | context.read metadata and separately gated closed context.progress.read projection, never private task detail |
| GET `/work-contexts/{id}/history` | getWorkflowHistory | context.history.read closed progress history; reason/body/identity separately authorized |
| GET `/tasks/{id}/working-artifacts` | listWorkingArtifacts | Current authorized attempt-private records |
| POST `/tasks/{id}/working-artifacts` | createWorkingArtifact | New private schema-valid draft metadata; shared inputs use explicitly classified InputResourceRef |
| PUT `/working-artifacts/{id}/content` | writeWorkingArtifactContent | Bounded8MiB binary upload, operationId/expectedRevision, immutable generation/hash; private until submit |
| GET `/working-artifacts/{id}/content` | readWorkingArtifactContent | Work/provider-current-authorized original attachment; bounded/range generation binding, no static URL |
| PUT `/working-artifacts/{id}` | updateWorkingArtifact | Revision-checked schema-valid private draft metadata update; completed snapshot unaffected |
| GET `/handoff-snapshots/{id}` | getHandoffSnapshot | Authorized immutable submitted membership, provider checks before content |
| GET `/return-instructions/{id}` | getReturnInstruction | Authorized immutable return context |
| GET `/tasks/{id}/workspace` | getEffectiveWorkspace | Managed/explicit/current-policy bindings and runtime readiness |
| POST `/workspaces` | createWorkspace | Name plus operation identity; runtime completion is explicit |
| POST `/workspaces/{id}/runtime-receipts` | confirmWorkspaceRuntime | Record advisory bound runtime observation; it grants no local authority or shared eligibility |
| GET `/workspaces/{id}/bindings` | listResourceBindings | Current accessible/enabled distinction |
| POST `/workspaces/{id}/bindings` | attachResourceBinding | Qualified provider/native receipt; no absolute path/credential |
| POST `/bindings/{id}/detach` | detachResourceBinding | Remove explicit reference only; managed/policy-derived rules preserved |
| POST `/bindings/{id}/enabled` | setResourceBindingEnabled | Exploration preference only, no ACL change |
| GET `/tasks/{id}/evidence` | listEvidence | Authorized bounded EvidenceRecord projections |
| POST `/tasks/{id}/evidence` | registerEvidence | Human/provider-origin provenance and policy-permitted reference/fragment |
| GET `/evidence/{id}` | getEvidence | Recheck current source/visibility; honest unavailable/partial |
| GET `/tasks/{id}/findings` | listFindings | Claim/support separated |
| POST `/tasks/{id}/findings` | registerFinding | Authorized Evidence refs; immutable candidate revision |
| GET `/findings/{id}` | getFinding | Candidate plus authorized support status |
| POST `/findings/{id}/decisions` | recordHumanDecision | HumanInteractive accepted|modified|rejected; preserved original |
| GET `/findings/{id}/decisions` | listHumanDecisions | Authorized immutable decisions/history |
| POST `/tasks/{id}/agent-executions` | requestAgentExecution | Bounded purpose/resources, current context; no principal override |
| GET `/agent-executions/{id}` | getAgentExecution | Authorized lifecycle/partial outcome |
| POST `/agent-executions/{id}/cancel` | cancelAgentExecution | Stop future work; unknown side effects remain explicit |
| GET `/agent-executions/{id}/result` | getAgentResult | Structured references, never transcript-only business state |
| GET `/generated-artifacts/{id}` | getGeneratedArtifact | Private candidate content/provider reference under current scope |
| GET `/suggested-actions/{id}` | getSuggestedAction | Non-executable typed proposal and support |
| GET `/operations/{operationId}` | recoverOrganizationOperation | Same actor/current authority and bound target/digest; no outcome oracle |

WorkflowDefinition/WorkStep/Transition/Profile and synthetic context/task creation
are versioned fixture/config inputs in v0, not an unrequested general workflow
designer/admin UI. Authorized runtime mutations use the API above. Synthetic
seeding uses a dedicated application adapter with explicit disposable ownership,
never production SQL or frontend-created credentials.

## 14. Transaction, error and resource limits

Work mutation, workflow history, operation ledger and required business/Audit
staging are one atomic Work transaction. Same operation ID+same command digest
replays one committed outcome after current authorization; changed digest yields
OPERATION_CONFLICT. OCC mismatch yields REVISION_CONFLICT, without applying
part of submit/return/reassign. Unknown commit exposes COMMIT_OUTCOME_UNKNOWN;
recover exact operation before a retry, never generate a replacement operation.

Lock order: workflow instance → affected WorkItems sorted by UUID → current
attempts/assignments → artifacts/decisions sorted by UUID → operation/event
records. Role/policy validation is bound through revision-checked authorization
conditions within the transaction; policy writers must participate in the same
fencing scheme. No remote call while holding a business-row lock. Provider
preflight before locking followed by current/freshness validation before commit
must not be advertised as a cross-provider transaction.

Operational admission: JSON request/response≤1MiB, page≤100, workflow definition
≤100 steps/200 transitions, context modules≤16, action/resource selection≤100,
handoff artifacts≤100, Finding evidence refs≤100, claim/reason≤8KiB each,
retained Evidence fragment≤16KiB and cumulative returned fragments≤256KiB.
UTF-8 byte bounds apply after schema parsing with bounded body admission;
reject duplicates/over-limit references before provider fan-out. Server provider
fan-out≤8, explicit timeout/cancellation via existing hierarchy, no unbounded
auto-retry. These are resource profiles, not business quality/latency promises.

Reuse VALIDATION_FAILED, AUTHENTICATION_REQUIRED, FORBIDDEN, REVISION_CONFLICT,
OPERATION_CONFLICT, CURSOR_STALE, DEPENDENCY_UNAVAILABLE, TIMEOUT,
COMMIT_OUTCOME_UNKNOWN and INTEGRITY_VIOLATION. Add closed codes only where needed:
WORK_ITEM_NOT_FOUND, WORK_ARTIFACT_NOT_FOUND, WORK_CONTEXT_NOT_FOUND,
EVIDENCE_NOT_FOUND, FINDING_NOT_FOUND, WORK_CONTEXT_STALE,
WORK_ASSIGNMENT_CONFLICT, HANDOFF_NOT_READY and RUNTIME_CAPABILITY_UNAVAILABLE.
No human-message parsing. Error payloads use safe codes/trace IDs and field JSON
pointers, without private title/path/body/actor disclosure.

## 15. Audit, business history and future integration

Work workflow history and operation ledger remain authoritative business records.
Required security/work mutation evidence is staged atomically, unsampled, with
actual principal plus acting role/assignment/delegation IDs, action, typed resource,
result, operation/request/trace correlation and bounded reason code/reference.
User-authored return/decision reason belongs to the authorized immutable Work
record; Audit may reference its stable retrievable record under separate access
policy, not silently replace required existing Document free-text reason.

PR45 A2's versioned schema is reference material for a future reviewed extension,
not an import of its unqualified store/delivery or a statement that normal ACL/
legacy reason preservation is repaired. Before connecting producers, add a closed
versioned Organization metadata catalog after Phase1/2 freeze and independently
qualify actual source→sink delivery in the Audit track. No complete Audit-pipeline
claim until those receipts exist. Audit outage after successful staging does not
undo committed Work; inability to create mandatory staging prevents commit.

Search integration is a later provider adapter contract: authorized discovery
reference/coverage/freshness in, current provider authorization before use out.
No WIP branch edits/import, ranking reimplementation, secrets/paths or automatic
retention of NO_RETENTION material. POWER EGG/MCP/business systems remain named
provider interfaces until separately connected and qualified.

## 16. Synthetic identity and fixture contract

Synthetic fixture principals are `sales-01`, `office-01`, `review-01`,
`approver-01`, `multi-role-01`, `delegate-01`, `agent-01`; IDs are explicitly
synthetic. Units 営業店/事務/融資審査/承認 provide defaults, not automatic ACLs.
Roles are sales, processing, reviewing, approving; multi-role has two formal
assignments; delegation is bounded and expires/revokes in tests. Agent remains a
distinct verified principal/invocation, never a Human header override.

Use process-fixed allowlisted `organization-synthetic` profiles at startup of
isolated Organization test servers; the configured profile selects only the
fixed fixture mapping, never arbitrary caller-supplied principal/group values. Request data cannot select principal/groups; role
selection is checked against the actual profile. Unknown/production modes fail
closed. Existing Document PoC profile behavior stays frozen. Shared synthetic
Document fixtures and grants are prepared only through existing allowed APIs;
no production group mapping or identity provider is invented.

Sales case and office RoutineRun share the same model. Definition fixtures cover
forward sales→office/review→approval, return/rework attempt2, private draft vs
snapshot, current assignment change, local artifact promotion, Human-origin
Evidence, multiple-support Finding, accepted/modified/rejected HumanDecision,
and both Agent-assisted paths. Business-specific decision text is fictional;
technical evidence records whether executor behavior is simulated or actual.

## 17. Review/acceptance matrix before implementation

| Area | Required counterexample and expected result |
|---|---|
| Shared authority | Same WorkItem IDs/revisions in context and queue projections |
| Context continuity | Sales sees only authorized context title/progress/history across downstream assignment; no private fields through filter/count/cursor |
| Draft provider bypass | Next/old assignee known IDs, direct Document/provider API, list/history/range/cache attempts never reveal new Work-private bytes before submit or after reassignment; shared inputs are never falsely labeled private |
| Queue privacy | Eligible non-assignee sees only explicitly allowed queue metadata; detail/list filters/counts cannot disclose private work |
| Claim race | Two eligible claims for same revision yield one assignment and one conflict |
| Revocation/time | Delegation expiry, role revocation and reassignment between read/commit deny stale mutation and later disclosure |
| Handoff | Snapshot/source completion/next-ready/event ledger commit together or not at all; no local-only/pending provider reference |
| Return | Attempt1/submission unchanged; new attempt2/private draft and causal reason visible only to authorized parties |
| Provider changes | Snapshot membership/old search result cannot bypass current provider denial; unknown auth cannot become success |
| Decision | Agent cannot record HumanDecision; modified preserves original candidate and separate adopted claim |
| Agent context | Role/task change or cancelled execution fences stale output; hidden draft never enters next-step Agent context; requester denied/executor allowed and requester allowed/provider-principal denied both reject disclosure |
| Synthetic identity | Header/body/tool principal override rejects; original MCP still requirespoc/poc-agent; agent-01/requestedBy/provider identity remain explicitly distinct |
| Runtime broker | Absolute/traversal/UNC/device/ADS/symlink/junction/reparse/TOCTOU, cross-principal/device/window IDs, same-inode mutation/stale read handles and limit+1 reject |
| Retry | Response loss reuses same operation/digest with current auth; changed input conflicts; no duplicate roots/submissions/tasks |
| Browser parity | Same React feature/API business semantics; unavailable native capability stays explicit, no fake local access |
| Evidence | Multi-source coverage/uncertainty/provenance retained; NO_RETENTION policy not bypassed by Evidence/Audit/Chat |
| History/Audit | Mandatory staging failure rolls back; Audit delivery remains separate and unqualified until its track completes |

These are design acceptance obligations, not executed tests. Phase3 must map
them to concrete interactions before Phase4 qualification and Phase5 TDD.

## 18. Delivery graph and next freeze

D1 (this Phase0/1/2 docs+authority) stacks directly on H2. D2 adds only two
concrete source designs and interaction/state/keyboard/accessibility mapping.
T1 follows D2 and qualifies official current Tauri/version/license/security and
actual supported runtime, including Windows10Pro/WebView2 and same-source web.
It must make any desktop-only CI/platform scope explicit; current Linux/backend
policy remains unchanged until its scoped amendment is reviewed.

Then implementation Draft units: W1 Work/Organization core+repository; W2
HTTP/OpenAPI/generated client; C1 shell+tasks; C2 preserved Document feature
integration; T2 bounded Workspace runtime; A1 Agent/Evidence integration; E1
synthetic integrated acceptance. W1 precedes W2; C1 uses W2 and frozen D2; C2
uses C1 plus inherited Document; T2 depends on qualified T1 and Workspace API;
A1 depends on W2/C1/T2 resource contracts and existing read-only MCP; E1 validates
their combined exact tree. Parent coordinates one linear reviewable integration
stack where needed, no PR merge/close. Task-level RED→GREEN and independent
reviews precede exact-head hosted gates. Plans must pin actual paths/interfaces/
commands after Phase3/4 findings, not invent an installed Tauri version now.

No new global owner-decision STOP is identified by this document. The original
STOP list remains intact; any finding that requires new major semantics is
reported before freezing or implementing that affected boundary.

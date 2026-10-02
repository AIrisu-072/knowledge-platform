# Organization Client v0 — Phase 1 Product / UX Design

Status: WRITTEN / INDEPENDENT PHASE 1 REVIEW PENDING. No Phase 2 freeze, product implementation or Tauri qualification is claimed.

## 1. Intent, authority and predecessor

Organization Client is the Human-facing operating surface for Human / Human /
Agent work on common Task, Workflow, Resource and Knowledge Context. It elevates
the existing Document GUI into a Document feature rather than replacing Document
Platform. The owner supplied the detailed request §§0–51 and expressly permits
faithful formalization and progression without repeated approval in §50. New
major semantics and the enumerated STOP conditions remain decisions for the
owner. This is an architectural track with separate sequential written freezes.

The exact accepted source is PR43 H2
`6103e4d4e3bb0d45ba03e1d2935492de7f11394a`, tree
`f2e13eee0d7e1bfa71952c1da52a72cecb65fc9e`. The
[owner acceptance receipt](https://github.com/AIrisu-072/knowledge-platform/pull/43#issuecomment-5952167525)
clears the requested predecessor. See the [Phase 0 inventory](../execution/organization-client-v0-phase0-reconstruction.md)
for current-head receipts, C0/Search/Audit limitations and unchanged-source boundaries.

Success means the same work can be understood and advanced through either of
two task projections; responsibility, current authorization, submitted work,
private work, evidence and human judgment remain distinguishable. A convenient
screen, Agent assertion or chat message never establishes authoritative success.

## 2. Selected composition and alternatives

Selected: one Organization shell, one shared WorkContext × WorkItem authority,
two WorkViewProfile base archetypes, reusable cross-cutting Context Modules and
the existing Document feature. The Runtime Contract presents the same React
frontend on Tauri v2 Desktop (primary) and Browser. This directly formalizes the
owner's chosen boundaries.

Rejected alternatives: a Document application with Tasks bolted into its folder
tree would confuse Workspace/WorkContext with Document folders; independent
sales/review/Agent applications would duplicate work authority and violate the
two-archetype constraint. Neither alternative is a new choice requested of the
owner. Tauri suitability is still Phase 4 qualification, not proven by selection.

## 3. Product and information architecture

Primary navigation has exactly **タスク / 文書 / 検索**. Landing is **タスク**.
案件, Workspace, Agent and Evidence are never universal primary entries.
Domain `WorkItem` is labeled タスク in Human UI. Search Platform retains its
name and existing architecture; Japanese 検索システム is allowed. No Discovery
Platform rename, RAG-only system or separate Human/Agent business API is created.

The five semantic regions are:

| Region | Responsibility |
|---|---|
| Application Shell / Global Navigation | Product navigation, verified current responsibility and runtime capability indication |
| Task Collection | Context projection or WorkType queue; stable selection and attention |
| Work Surface | Current task's principal work, draft or submitted artifact |
| Context Surface | Evidence, Agent Chat, Document, Diff, Search results, histories, return instruction and related resources |
| Action Surface | Authorized workflow actions: 完了, 提出, 差戻, 保留, 担当変更 |

Action availability is a backend hint and never a permission. Submission,
handoff, return, assignment, HumanDecision and permission-affecting operations
show immediate Pending and only authoritative confirmed success. Unknown
completion remains unknown and recoverable; neither an animation nor an Agent
message commits work.

## 4. Exactly two archetypes

| Feature UX Context | 営業型 / context | 事務型 / queue |
|---|---|---|
| Task Context | Sustain customer/case/request continuity and determine the next action | Process assigned/eligible WorkItems by WorkType, including review/approval |
| Frequency | Repeated visits to a selected context; bounded related tasks | Repeated sequential task processing; counts/queues are projections |
| Required Information | Context identity, current progress, own next work, responsibility, resources, evidence/history | WorkType, minimal queue identity/attention, assignment, due state, current task/input/evidence and allowed action |
| Decision | What is happening and what should I do next? | What may I process now and what evidence supports the action? |
| Error Consequence | Wrong context, stale advice, unsubmitted draft mistaken for shared work | Wrong target, unauthorized disclosure, stale claim, incorrect decision or duplicate handoff |
| Primary Interaction | Context selection → related task → work/context review → confirmed action | Queue selection → claim/open → work/context review → confirmed action → explicit next item |

Sales projects **WorkContext → related WorkItems**. Office projects **WorkType →
WorkItem Queue**. They are views of the same records, not different domains.
Review is a queue-based WorkViewProfile that gives Evidence/Document greater
priority. Return is a workflow transition plus attention/rework context, never
a new layout type. Evidence and Agent usage do not create additional archetypes.

Selection is bound to a stable identity, never row position. Refresh, sorting,
claim races and permission changes cannot silently switch the selected target.
Returning from Document/Search restores valid collection context; revoked or
removed items are explicitly reconciled rather than resurrected from cache.

## 5. WorkViewProfile and Context Modules

WorkViewProfile describes `base_archetype(context|queue)`, primary/secondary
grouping, default sort, filters, visible information, density, context modules,
module priority and default actions. Department defaults and role-specific
overrides may optimize presentation but never widen access or invent actions.

Context Modules are Evidence, Agent Chat, Document Viewer, Diff, Search Results,
History, Workflow History, Return Instruction and Related Resources. A profile
and current task may request `hidden / available / visible / prominent`; these
are presentation states, not authorization states. Unauthorized content never
arrives merely because a module is hidden. A prominent module may remain
Unavailable or explicitly partial. Evidence is available in both archetypes and
quickly reachable even when not occupying the principal Context Surface.

Density derives from the task's simultaneous-information needs. Review may
prioritize evidence and originals, while sales prioritizes context continuity.
No universal card dashboard, unconditional full-density table or separate review
layout is selected. Concrete dimensions and interaction source follow Phase 2.

## 6. Responsibility, eligibility and private work

OrganizationalUnit provides candidate pools, default profile, available roles and
resource-policy defaults. It is not the direct Task authorization authority.
BusinessRole/Responsibility owns policy; RoleAssignment binds a Principal over
`valid_from / valid_until`. Temporary Delegation remains distinct from formal
assignment. UI explains actual principal and current acting responsibility;
Audit records both without storing identity display names as a new authority.

Eligibility means a principal may be considered for a task. Assignment means
they currently own the concrete task responsibility. Queue visibility is a
separate minimal projection and does not disclose assigned private content to
every eligible colleague. Effective access depends on role eligibility,
assignment/responsibility, workflow state, artifact visibility and provider-side
current authorization. Search discovery scope and client capability are hints.

Working artifacts use `work_item_private`; submitted Handoff Snapshots use
`handoff`; deliberately shared context resources use `context_shared`.
Visibility must be enforced by Backend/API and provider authorization, including
direct IDs, lists, previews, Evidence, Agent context and cached-result disclosure.
Frontend hiding is never the boundary. A next-step assignee cannot read the
upstream draft before submit, including a replacement draft after return.

## 7. Handoff, return and attention

Forward handoff is current WorkItem completion → immutable Handoff Snapshot →
next WorkItem ready. It transfers submitted work references, not copied grants
or a local physical path. The recipient starts with that snapshot under current
authorization. Provider content that became unavailable is shown honestly;
snapshot membership does not grant ongoing provider access.

Return preserves previous submission, reason, causal link and attempt number;
new work gets a new activation/attempt and a new private working draft. Completed
history is never reopened and overwritten. Phase 2 compares stable WorkItem+Run
against activation records and selects the smallest model implementing this
already-approved invariant.

`newly_assigned / returned / due_soon / overdue / waiting_for_confirmation /
blocked` are derived Attention Projection inputs. They do not multiply Domain
lifecycle values or equate “attention cleared” with work completion. No arbitrary
deadline threshold, auto-claim, auto-advance or irreversible bulk workflow action
is invented; contextual due semantics and bounded policy belong to Phase 2.

## 8. Evidence, Finding and HumanDecision

Evidence is a verifiable basis for judgment. EvidenceRecord can retain a source
reference/type, authoritative locator, relevant location/allowed fragment,
Document Revision/Version, retrieval time, provenance, coverage, uncertainty and
conflict. It is not synonymous with a Search result, model output or copied
source body. Missing provenance/coverage is explicit, never filled by guessing.

Finding is a candidate claim/interpretation supported by one or more Evidence
records. HumanDecision records accepted/modified/rejected judgment with its
actual Human actor and selected Finding. Accepted Finding is not proof of an
external fact; the decision and original candidate remain separately inspectable.
Modifying or rejecting does not rewrite the original Evidence or prior decision.

Human, Document Platform, Search Platform, Agent, MCP Resource, POWER EGG,
external sources and business systems may supply Evidence through an authorized
provider contract. No external connection is created by listing a provider.
Evidence bodies obey provider retention/authorization restrictions; a reference
can become unavailable without being presented as verified current content.

Both archetypes support: candidate → supporting Evidence → original/location →
Human decision → explicit next action. Provenance/version/coverage/uncertainty is
shown next to the claim it qualifies, not hidden only in chat or a hover tip.

## 9. Agent Chat and structured execution

Agent Chat is a cross-cutting Context Surface attached to current task/context,
not a primary navigation entry or third archetype. It may receive current
authorized WorkContext, WorkItem, Responsibility, Effective Workspace, Resources,
Tools, relevant history and Evidence. Changing role/task/revoked rights requires
fresh context; stale frontend caches and transcript text cannot grant access.

AgentExecution may produce Result, Finding[], Evidence[], GeneratedArtifact[],
SuggestedAction[] and links to HumanDecision[]. The Human can accept, modify or
reject candidates. Business records remain Task/Artifact/Finding/Evidence/
HumanDecision/WorkflowTransition records. Transcript is interaction history only;
important results cannot exist exclusively there. An Agent suggestion is not a
workflow execution or Human decision; no model/provider/credential or broad tool
execution is selected in this Phase. Existing Document MCP remains read-only.

Sales journey: current customer/case → authorized investigation request → Finding
→ Evidence check → HumanDecision → explanation/document/next action. Office
journey: queue task → authorized check → Finding → Evidence check → HumanDecision
→ input/review/approval/return. Both preserve origin and uncertainty.

## 10. Logical Workspace and resources

Workspace is logical working context + managed resource + explicit bindings +
policy-derived bindings, not a Folder. It normally resolves from current Task.
One creation flow asks for name and optional accessible local folders; omitted
folders cause the capable runtime to create a managed root. An existing folder
is attached as a resource, never converted into the Workspace itself. Names and
physical paths are independent.

Bindings may be managed, explicit or policy-derived. Effective identity, active
roles, unit, WorkContext and WorkItem derive policy bindings; movement/revocation
must not leave old effective access. Accessible differs from Enabled in Workspace:
enabled controls ordinary exploration/use, not authorization. Neither disabling
nor enabling changes provider grants.

Local folders are principal/device-local. Shared artifacts must be saved/promoted
to Document Platform or another qualified shared authoritative provider before
handoff. Local bindings/paths cannot be inherited by a different principal or
device as shared resources. Upper Domain/Search/Agent use opaque binding IDs and
logical locators, with no credentials, physical paths or execution secrets.

## 11. Runtime, Document and Search boundaries

The same React frontend consumes RuntimeCapabilities, WorkspaceRuntime,
LocalResourceCapability and NativeDialogCapability. Components do not scatter
Tauri conditionals. Browser File System Access adapter is future/low priority;
unsupported capability is explicit rather than desktop UX being weakened.

Tauri v2 Rust exposes only bounded operations such as create_workspace,
attach_directory, detach_directory, list_entries, read_file and create_file.
No arbitrary absolute paths, shell/executable launch, traversal, symlink escape
or request-supplied privilege. Exact bounded contracts and tests follow Phase 2
and official/runtime qualification Phase 4. v0 is single main window with routes,
panels/splits/dialogs/tabs; no pop-outs or sidecar. Future sidecar boundary is
reserved, not implemented.

Reuse Document React routes/feature, client/bridge, Version/Revision/OCC,
lifecycle, backend capabilities, authorized Diff/originals, focus, motion and
error semantics. Shell integration does not replace Document authority or add
frontend parsers. Document publication remains distinct from workflow handoff.

Search Platform stays independent and supports future discovery of information,
resources and tools through its existing boundary. Discovery ≠ Authorization ≠
Execution. Search WIP is untouched and never qualified by this work. External
provider names are future integration contracts, not connected features.

## 12. State, accessibility and verification contracts

Phase 3 must show the same two layouts for normal, newly assigned, returned,
working draft, handed off, due soon, blocked, Agent-active, Evidence-review and
Document-comparison scenarios. It must also specify Initial/Empty/Loading/
Pending/Ready/Partial/Stale/Conflict/Error/Unauthorized/Unavailable/Disabled.
Zero rows never means an unreadable queue; no hidden failure or partial-as-full.

Preserve `spec/requirements/frontend-ux-requirements-v0.md`: WCAG2.2 AA,
keyboard-only principal flows, visible/restored focus, stable ID selection,
context-preserving navigation, non-color status and risk-based confirmation.
Reuse motion0/90/140/180ms and instant reduced motion; animation never blocks
next work or establishes success. The normative UX budgets remain input feedback
≤50ms, local usable/keyboard≤100ms, motion blocking0ms,60fps target and long tasks
normally<50ms. Backend time is measured separately, not assigned an invented SLO.

Synthetic evaluation includes sales/office/review/approval units, multiple
principals, concurrent roles/delegation, context and routine, handoff/return/
private draft/snapshot/reassignment, Evidence/Finding/HumanDecision and both
Agent-assisted journeys. Source design observation is not actual runtime proof.

## 13. Sequential gates, non-goals and STOP

Order is fixed: Phase0 reconstruction → Phase1 Product/UX freeze → Phase2
Domain/API/Auth freeze → Phase3 concrete UI/interaction freeze → Phase4 Tauri
qualification → Phase5 implementation → Phase6 synthetic evaluation. Phase2/3
cannot be swapped; backend implementation waits for UI semantics/source freeze.
Separate independent reviews and exact blob records close each boundary.

Production identity is deferred (no AD/Kerberos/Entra/SSPI speculation), while
synthetic identity must test multiple principals/roles/units/delegation. Existing
Linux backend/development/CI restrictions remain in force. A separately explicit
Desktop qualification scope will be evaluated in Phase4. That phase must verify
the current official stable Tauri v2 release, direct/transitive licenses and
security, Windows 10 Pro viability and WebView2 requirements, existing React/
Webpack compatibility, and the requested actual runtime proof. No global Windows flag
or security/license relaxation is made here.

Stop only affected work for: incomplete PR43 acceptance; new major business
semantics; production identity choice; broken Document authority; Search
architecture change; license/security mismatch; Tauri unsuitability; local-as-
shared fiction; frontend-only draft security; unavoidable loss of Evidence
provenance; separate Human/Agent business logic; compulsory old-PR merge/close;
production deployment. Ordinary file/crate naming, internal refactors and test
structure are not owner-decision gates. New credential/persistent-access,
installation and data-sharing approvals remain subject to their safety policy.

No current new global STOP was found. Audit schema qualification is not store/
delivery completion; Organization defines a future integration boundary without
depending on unqualified A3 code or using Audit as analytics/personal memory.

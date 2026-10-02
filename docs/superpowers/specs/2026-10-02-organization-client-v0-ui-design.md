# Organization Client v0 — Phase 3 concrete UI / interaction design

Status: SOURCE WRITTEN / INDEPENDENT REVIEW AND ACTUAL PIXELS PENDING.
This is an inspectable synthetic source design, not product implementation or
backend/runtime/security qualification. Phase4 has not started.

## 1. Frozen inputs and source identity

[D1 Draft PR47](https://github.com/AIrisu-072/knowledge-platform/pull/47) remote
`13c1292c806c9179be0a444ef2b4be8234e00bf4`, tree
`354a8ac2fbd2642afbb9b53518e22461642cec70`, is tree-equivalent to local
`ac76156ecba5a139a1df9ead5e00616aa08aa5af`. Its parent is accepted H2. D2's
local branch uses that equivalent tree; publication maps to the actual remote
D1 parent, not an invented remote identity.

The frozen [Product/UX](2026-10-02-organization-client-v0-product-ux-design.md)
blob `a2901ccb866fc85b18301db27dd66aa629791201` and
[Domain/API/Auth](2026-10-02-organization-client-v0-domain-api-design.md) blob
`9f68bf19eb9986eb1c78082704a5d38ff35e35af` are unchanged. Phase1/2 independent
GO preceded this source. Original§50 permits faithful progression; a new major
semantic choice is still STOP.

Source directory: `docs/design/organization-client-v0/`.

- `sales.html`: complete context-centric sales entry
- `office.html`: complete queue-centric office entry
- `prototype.css`: shared operational tokens/layout/focus/reduced-motion rules
- `prototype.js`: bounded local-only interaction preview
- `scenarios.json` and exact generated `scenarios.js`: ten shared states
- `source-design.test.mjs`: dependency-free source/shape boundary checks
- `interaction.test.mjs`: optional DOM unit checks using existing qualified jsdom

There are exactly two archetype entries. Review controls above the app select a
synthetic state and link the two entries; these controls are not production
navigation. Prototype operations are clearly marked previews and do not contact
servers, native files, Search or a model. No third-party/CDN assets are loaded.

## 2. Concrete shell and visual system

Desktop baseline1440×900; also qualify1280×900. The shared shell uses126px global
navigation,238px collection, flexible Work Surface and364px Context Surface.
At≤1320px those side widths become106/204/332px. Main tracks use `minmax(0,1fr)`;
long labels/timezones wrap. Full-page source capture may exceed the900px viewport;
that must be labeled and bounded, not described as a fixed viewport image.
Below1080px Context Surface moves below Work/Collection; mobile is not a new
formal support target or a third archetype.

Primary links are only タスク/文書/検索, with タスク current at landing. Header
names actual synthetic principal and acting responsibility separately. The
logical Workspace and Agent/Evidence are Context Modules, never extra primary
entries. Context and queue are not persistent data forks.

Reuse the inherited Document light neutral palette, Japanese font stack,13px
body, visible focus,6px controls and motion0/90/140/180ms vocabulary. No new font
dependency. Reduced motion suppresses motion without dropping information.
Review must check actual Japanese glyphs, weight hierarchy and wrapping; source
CSS alone does not prove readability. State uses text/position/grouping and
icons where useful, never color alone. Headings/landmarks/control labels are
semantic HTML; Context selectors are ordinary pressed-state buttons, not an
incomplete ARIA tab pattern requiring undocumented arrow behavior.

## 3. Sales source — context continuity

The collection shows authorized WorkContexts. Work Surface identifies selected
context, current progress, own next action, responsibility and related step
progress; the editable task belongs to that same stable context/item selection.
Context continuity uses `context.read/progress.read/history.read`, never an
overbroad interpretation of task-private read. Downstream progress may be shown
while downstream drafts, sensitive reasons, identities and Evidence bodies are
absent unless separately authorized.

Example synthetic path: サンプル商事 → 設備更新相談 → 内容確認 → related step
progress → Agent investigation → Finding → Evidence/original → HumanDecision →
draft explanation/document → explicit submit. Other collection entries are
synthetic and selection changes the visible target; draft state is keyed by
stable item identity. Old Finding/Agent previews are cleared when the task changes.

## 4. Office source — WorkType queue

The collection first selects WorkType, then shows stable WorkItem rows for that
type and current responsibility. Ownership filter distinguishes assigned work
from eligible-only minimal queue records. Different WorkTypes use distinct
stable item identities; changing a filter never mutates a task's WorkType.
Review is the same queue archetype with Evidence/Document prominent.

An eligible-only row discloses a generic type and claim possibility, not customer,
draft, Evidence or Agent context. Work/Context surfaces show the non-disclosure
state until claim wins. Claim confirmation does not assert assignment before
server success; competing claims remain conflict on the same selected target.

Example synthetic path: WorkType内容確認 → assigned task → submitted snapshot →
Agent check → Finding → Evidence/original → HumanDecision → review/input →
explicit submit or return. Sequential next-item navigation must be explicit;
background refresh or action completion cannot silently replace the selected task.

## 5. Shared context modules

| Module | Concrete presentation | Contract preserved |
|---|---|---|
| Evidence | Candidate panel, separate Evidence records, source/version/time/locator/coverage, original action, separate HumanDecision | Finding is a claim, Evidence is support, HumanDecision is actual Human judgment; Human-origin evidence first-class |
| Agent Chat | Current-task request, execution state, structured result action, expandable identity/scope explanation | requester/executor/provider identities distinct; no transcript-only record or Agent-authored HumanDecision |
| Document/Diff | Official Revision vs content Version, old/new comparison, Partial/uncompared region, originals | Reuse existing feature/client/bridge; no frontend parser or altered Diff/ACL semantics |
| Search | Task-context query, source-located result and Partial warning, explicit Evidence registration path | Search Platform name retained; discovery is neither permission nor execution; WIP not connected |
| Return | Prior snapshot, immutable reason/cause, attempt2 and private new draft | Return is workflow/rework state, not a new layout or reopening of history |
| History | Authorized progress/transition/submission sequence with explicit timestamps/zones | Business history distinct from Audit; no unauthorized reason/identity/body disclosure |
| Related Resources/Workspace | Managed/explicit/policy-derived sources, local/shared distinction, Accessible vs Enabled, one creation dialog | Local path never inherited; new private bytes Work-owned; shared Document reference is an input, not relabeled private |

Comparison is an orthogonal presentation mode: opening/closing it cannot reopen
a submitted task or remove received ReturnInstruction. Outbound return closes
the current task and shows its instruction read-only; it never exposes the
recipient’s new private draft under the returning actor. Only the external
review selector switches synthetic state fixtures.

Module priority changes which module is initially visible/prominent. It never
changes authorization. Opening one preserves the Work draft; changing context or
effective identity fences/clears old private response state before rendering.
Provider revocation produces explicit unavailable/denied content, not stale
body from a cache or silently omitted evidence claimed complete.

## 6. Ten states, same two layouts

Each row is rendered in both sales and office; only selected context/queue
projection and profile emphasis differ. No new layout archetype is introduced.

| State | Work/Action behavior | Context emphasis |
|---|---|---|
| normal | Current task and private working memo, capability-derived actions | Evidence available; candidate/decision separated |
| newly_assigned | Explicit new assignment attention, submitted starting material | Authorized history; no permission inferred from department |
| returned | Prior attempt stays completed; new attempt2/private draft | ReturnInstruction, prior submission, causal link |
| working_draft | Work-owned generation/private label; next step cannot access | Resources/local-vs-shared inputs; no provider privacy fiction |
| handed_off | Read-only pinned submission membership, next progress only | Handoff/history; no next-step draft or old editable pointer |
| due_soon | Explicit due instant/zone and policy-derived attention | Evidence/prerequisites; no invented universal lead threshold |
| blocked | Retained input; submit unavailable with reason | Current provider unavailable; not empty/success |
| agent_active | Work remains independently operable; execution not task completion | Current task Chat plus structured-result path |
| evidence_review | Candidate→sources→actual HumanDecision | Coverage/uncertainty and modified/rejected outcomes |
| document_compare | Bounded old/new content workspace; Partial is explicit | Version/Revision/original links, no false unchanged |

## 7. Interaction contracts and API mapping

| Interaction | Local behavior and authoritative boundary |
|---|---|
| Select/filter task | Stable ID, contextual URL/filter restoration; `listWorkItems/getWorkItem/getWorkContext` current projection, never stale row-index selection |
| Claim | `claimWorkItem` with current attempt/OCC/acting responsibility; Pending then assigned or conflict; private content only after authorized response |
| Edit private draft | `updateWorkingArtifact` / bounded `writeWorkingArtifactContent`; retain input on errors, never write into an already-shared source |
| Submit | Show target attempt, pinned artifacts/Evidence/decisions and next responsibility; `submitWorkItem` creates immutable handoff and next-ready atomically |
| Return | Show prior submission and required reason; `returnWorkItem` creates new causal attempt; previous content remains inspectable and immutable |
| Reassign/hold | Separate currently authorized workflow operation, no implicit privilege copy; after reassignment discard now-unreadable private cached data |
| Human judgment | `recordHumanDecision` on exact Finding revision; modified requires adopted claim; original support preserved; no automatic workflow action |
| Agent request/cancel | `requestAgentExecution/cancelAgentExecution`; current dispatch scope and independent execution state; outcome_unknown retained after ambiguous cancellation |
| Evidence registration | `registerEvidence` with authorized source/provenance/policy; Search result never saved wholesale by default |
| Workspace | `createWorkspace` plus bounded runtime result; browser native unavailability is explicit and prevents false managed-root success |
| Local promotion | Explicit user-selected upload into private Work storage; local-only reference cannot enter handoff; separate Document promotion does not weaken private draft boundary |

Source demo confirmation changes only the preview and says no server operation
occurred. Production pending/commit/unknown/error behavior is specified here and
in Phase2; local DOM preview cannot qualify it.

## 8. Error and interrupted-flow matrix

- Initial/loading: identify what is loading; do not label it ready or use a stale
  prior task as the current context. Eligible-only placeholder contains no draft.
- Empty: zero authorized results after a successful query. Unavailable/failed
  query is never an empty state.
- Pending: immediate feedback for submit/return/claim/decision; disable duplicate
  invocation of that operation, not unrelated reading or module switching.
- Conflict/stale: keep entered data and stable target; show which state requires
  refresh. Do not replay under a new operation ID or silently apply to a new task.
- Unknown commit: show unknown/recovery action; same-operation outcome recovery
  under current rights precedes retry. No success animation substitutes.
- Unauthorized/reassigned: clear newly inaccessible draft/Evidence/Agent content,
  retain only safely permitted explanation, and restore focus to the nearest
  surviving control. No known-ID/provider bypass or existence-leaking reason.
- Partial: show existing useful results alongside coverage/missing-source status;
  do not reduce to whole-page error or call it full verification.
- Disabled/unavailable: explain current state vs lack of capability vs missing
  permission. Browser native folder/managed root cannot pretend to succeed.
- Dialog cancel/Escape: no request sent; preserve draft and return focus to the
  connected enabled trigger, otherwise Work heading. Native modal behavior is
  retained, with dialog-local Tab/Shift+Tab boundary wrapping; actual browser
  containment still requires hosted proof.
- Task/role switch: cancel/fence obsolete requests, clear old private outputs;
  no old Agent result attaches to the new task. Module switch alone preserves
  draft and current selection.

## 9. Keyboard and accessibility source contracts

Skip link→Work Surface; native Tab order follows navigation→collection→work→
actions→context. Collection rows are labeled buttons and selected stable ID is
visible/announced. Enter/Space activates the focused row; no custom OS shortcut
override. Module buttons support ordinary native keyboard behavior with
aria-pressed. Form labels and errors stay near the fields.

Dialog opens on Cancel for high-impact previews. Native modality/Escape/close
remain intact; while open, an unmodified Tab at the last eligible control wraps
to the first, and Shift+Tab at the first wraps to the last. Ordinary in-dialog
DOM order is native; disabled, nonrendered, hidden-visibility, inert and negative-
tabindex controls are excluded. The current dialogs contain no positive tabindex.
Ctrl/Alt/Meta shortcuts, other keys and closed-dialog events are untouched.
Escape/cancel restores focus, and action success/state changes are announced once
through polite status. Confirmation lists target/current attempt and outcome;
color alone never distinguishes private/handoff/blocked. Long timezone/offset
labels wrap. Reduced motion preserves semantic content. Actual browser tests
must check layout bounds/focus/keyboard/reduced motion; DOM emulation is not a
WCAG certification, pixel review or performance measurement.

## 10. Qualification state and next gate

Initial dependency-free tests were RED4/4 for missing source, then GREEN4/4.
Existing qualified jsdom26.1.0 DOM checks initially passed10/10; the reviewed counterexample suite now passes24/24 (including20 scenario renders,
selection/draft isolation, eligible-only non-disclosure, confirmed handoff preview
and separate modified HumanDecision). JS syntax/whitespace pass. No dependency
graph, product React implementation or server behavior changed.

Actual cloud-browser local navigation was refused with
`net::ERR_BLOCKED_BY_CLIENT`; no alternate route/bypass or user Mac was used.
No screenshot/pixel/browser rendering evidence exists from that attempt. The
owner separately approved a bounded GitHub Ubuntu D2 visual route: exactly
sales/office×10 synthetic PNGs, one-day public-repository Actions retention, the
existing exact pinned official uploader/graph exception for this D2 review only.
Source/workflow/privacy review and normal exact-head qualification must precede
the parent-only head-bound one-shot capture activation. PR43 is untouched.

Planned export:20 images at1440px, full-page bounded≤2400px and≤8MiB each.
1280/1440 geometry checks are non-recording. No video, trace, raw log, credentials,
extra screenshot or source/private file is uploaded. Actual readability and
interaction/layout findings must be reviewed after receipt, not inferred from
green DOM tests. Phase3 remains pending until the source/interaction and actual
visual gate are complete; Phase4 Tauri qualification remains after that boundary.

## 11. Actual-pixel correction amendment — 2026-10-02

The [first actual20-image review](../execution/organization-d2-visual-review-v1.md)
is NO-GO on captured bd1f57f4, with Important stale-handoff presentation and
misleading blocked Submit in both archetypes. This amendment clarifies already
frozen Product/UX§4 and Domain§4/7 semantics under original owner§50; no new
workflow, downstream permission or backend authority is added.

A selected completed WorkItem is historical/read-only even when its WorkContext
has a next-ready step. Handoff headings, own-next-work and summary must say
submitted/completed and offer submitted-content/history review; the rail must
show the submitted 内容確認 complete, current 審査 ready and 承認 still future.
Office's section must use completed rather than prospective framing. This is
the existing synthetic forward path, not a general derivation of arbitrary
workflow steps. Comparison and module/selection round-trips preserve both this
projection and exact immutable membership. Outgoing return uses completed own
work plus only the generic new ready return attempt already defined in Domain§4;
it does not invent a named recipient step or expose its private draft.

Blocked Submit stays disabled. Its immediately adjacent visible reason starts
「提出不可」, identifies the current blocking condition, and is associated by
`aria-describedby`; a normal state removes the stale reason/association. Disabled
primary controls, including Workspace Confirm, remain neutral and dashed-border
under hover/focus styling, without changing native disabled keyboard behavior.
Text plus shape/state association prevents color-only communication.

Normal hosted qualification must assert actual DOM state and disabled computed
styles/hover/focus, retaining all prior keyboard/font/geometry checks. These are
necessary source-design gates, not a replacement for authorized corrected
pixels and independent actual visual review. The original20-file capture
allowlist,1440-full-page limits, non-recording1280/1440 geometry, one-day
retention, owner/head/time/prerequisite/export guards and dependency locks stay
unchanged. Phase3 is not frozen and Phase4–6 remain unstarted.

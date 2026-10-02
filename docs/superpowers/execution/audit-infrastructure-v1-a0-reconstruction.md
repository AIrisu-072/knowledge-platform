# Audit Infrastructure v1 — A0 repository reconstruction

Snapshot: 2026-10-02 UTC. Read-only reconstruction completed before any product changes. Repository state and live GitHub receipts take precedence over stale execution prose.

## Base decision and evidence

Use `d71753d46590bb4406a1c0b74894ab90a27a6c88`, tree `0d5e1b473c9c0324b55d1171396a17d65e2a925c`, on independent `design/audit-infrastructure-v1`. This happens to be current main; it was selected after comparing the live Draft stacks, not by assuming main is suitable.

- Own [push CI 36718016267](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36718016267): SUCCESS, including all nine jobs and `required-check`.
- Identical tree to merged PR35 head `4b9df28d054526114dd8a48956d5ac4ddecd5f46`; [CI 36716092331](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36716092331), [Sandbox 36716092340](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36716092340), [DSI PoC 36716092268](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36716092268) all SUCCESS.
- Live GitHub confirms Core #3/#4, Publish #5/#6, Versioning #11/#12, Publication End #13/#14, Management #15–19, Diff #23/#24, HTTP #27/#29–32, and GUI design #35 merged. Older status files still saying Draft/pending are historical, not current GitHub facts.
- PR36 `706307b980beb540db0759ad772ed4debd42c289`: CI/Sandbox/PoC SUCCESS, but real GUI/composition acceptance remains unfinished. It adds no prerequisite for independent Audit delivery.
- PR37–39 are Draft design/plan layers for the PoC. PR41 `9510d15083753f35aa74da34890c1113ba8a7ef7` was still CI in progress; PR42 `ae4b1f12c235ef4bbfa151a9a1abc7d19d10b0d1` and PR43 `0aeba47f9e639705b9cc99def92b188d4a31f642` have failed CI. Their Sandbox/PoC successes do not qualify the complete heads. No code from these layers is required.
- Search #20/#21/#22/#25/#26/#28/#33/#34/#40 remain Draft. PR40 `a945fbd32145a3109e35cb9cb056cea052698138` has CI and DSI PoC FAILURE, Sandbox SUCCESS. No dependency on this WIP is accepted.

## Normative and approval inputs

Read `AGENTS.md`, `docs/superpowers/execution/active.md`, the pointed GUI status/design/plan, and the relevant sections of these normative files:

- `spec/architecture/architecture-contract-v0.md`: §§4.4, 7, 13; Audit owns append-oriented evidence, distinct from metadata transaction staging and Observability.
- `spec/operations/observability-audit-requirements-v0.md`: §§12.4–18, 19–22, 26–31, 33; unsampled mandatory evidence, CloudEvents1.0.x, atomic staging, asynchronous idempotent delivery, minimization, access, retention, integrity.
- `spec/data/transaction-consistency-requirements-v0.md`: INV-09/10, T1–T12, §7; source transaction and Audit creation share commit, delivery does not.
- `spec/data/logical-data-model-v0.md`: Audit separate schema/store; ReadState is current state, not Audit history.
- `spec/selection/library-tool-selection-v0.md`: PostgreSQL/SQLx are selected for metadata; SQLite production rejected; CloudEvents SDK is POC REQUIRED, Audit Store DEFERRED. Neither is silently promoted.
- `spec/selection/db-selection-criteria-v0.md`: permissive license, on-prem, concurrent writers, recovery, qualification rather than product-name assumption.

Frozen predecessors inspected: Authoritative Core design/plan and approval; Publish design/plan and approval; Versioning design/plan and approval; Management design `38010802a04c285336810e9b9c637c656ed1a76b`, plan `3b5cc84a8593134cdd7e01ea026bd2a124fa9585`; Diff design `afee4e9351c5027b1252e8c7b74e542a295f0e67`, plan `0bdbba832ebcfc2d211fddbba18dc31e69e1a28c`; their status and acceptance records. Existing Document contracts are not replaced by this track.

## Actual dataflow and schema

Application emits `AuditEventRecord` for create/initial Publish; repository producers cover subsequent Version, Publish, schedule/cancel/terminal, withdraw, publication end, metadata/move/Folder/ACL, first read confirmation, file access and Diff access. `authorization.denied` is recorded separately after denial; a recording failure never turns denial into success.

`crates/document-repository-postgres/migrations/0001_document_authoritative_core.sql` creates `audit_outbox_events`: event UUID PK, type, source, subject, actor issuer/principal, resource UUID/optional Version UUID, result, optional trace ID, JSONB data, occurred_at, attempt_count, delivered_at. Migration0006 adds typed Document/Folder/AccessPolicy resource. This table is staging, not a delivered Audit Authority.

Source writers are in repository `repository.rs`, `publish.rs`, `versioning_mutation.rs`, `withdrawal.rs`, `schedule.rs`, `publication_end.rs`, `targeted_events.rs`, `read_state.rs`, `file_access.rs`, `document_diff_access.rs`. Management callers use `document_management.rs`, `folder_management.rs`, `access_policy.rs`. Application event constants live in `document-application/src/events.rs`.

Real-PostgreSQL tests cover insertion rollback in `repository_contract`, `publish_transaction`, `versioning_transaction`, `withdrawal_transaction`, `schedule_transaction`, `publication_end_transaction`, `document_metadata_transaction`, `management_move_transaction`, `folder_management_transaction`, `access_policy_transaction`, `read_state_transaction`, `version_file_access`, `document_diff_access`, plus concurrency/vertical/event-matrix tests. File/Diff audit means permission to disclose, not successful network transfer. Mutation replay/no-op/read duplicate rules remain unchanged.

There is no Audit Store crate, delivery worker, schema artifact, integrity verifier, role policy, retention/export/backup implementation or end-to-end delivery receipt on this base. `spec/telemetry/audit-event.schema.json` is mentioned as future placement; the directory/file is absent.

## Capability matrix

Classification is for selected base, not a claim about deployed systems.

| Capability | Classification | Evidence / gap |
|---|---|---|
| Generation | implemented+qualified | Document event producers and tests above |
| Transactional Audit outbox | implemented+qualified | Mandatory inserts in business/disclosure transaction; rollback tests |
| Event schema | implemented incomplete | Record/SQL shape exists; no shared closed schema or evolution checks |
| CloudEvents | specified only | Envelope absent; SDK not qualified |
| Taxonomy | implemented+qualified for existing events | Document/Folder/ACL/security types exist; no BusinessEvent invention |
| Actor/resource attribution | implemented+qualified within Document | issuer/principal, typed resource; scheduler initiator distinct from serviceExecutor |
| Operation/request/trace correlation | implemented incomplete | Operation IDs unevenly embedded; Diff trace exists, many producers set NULL; no universal request correlation |
| Delivery | missing | No Audit dispatcher |
| Retry | specified only | attempt_count is unused state without a worker |
| Store idempotency | specified only | Source event UUID unique; no sink deduplication |
| Terminal/DLQ | specified only | No Audit failed-state implementation |
| Durable Audit Store | specified only | Deferred backend |
| Append-only | specified only | Staging has no business-role privilege boundary |
| Tamper/integrity | specified only | No Audit digest/checkpoint or reconciliation |
| Retention | specified only | No fixed years in normative contract |
| Investigation access | specified only | Separate from business authorization, no concrete port |
| Export | specified only | No export API/CLI |
| Backup/restore | specified only | Metadata capability is not an Audit restore receipt |
| Audit of Audit reads | specified only | Required extensibility; no implementation |
| Minimization | implemented incomplete | Most payloads use IDs/codes; arbitrary legacy reason and changing-key strings need explicit safe projection |
| Health | missing for Audit pipeline | No produced/delivered/stored/verified split |
| Reconciliation | missing | No source/sink comparison |
| Restart/recovery | missing for Audit pipeline | Source durability alone is insufficient |
| Pipeline Observability | specified only | No Audit delivery metrics/diagnostics |
| Org-specific attribution | blocked by Organization Client | Future WorkContext/roles/delegation are not prerequisites for core |

## Existing semantics and privacy wrinkles

Scheduler library already retains initiating actor and adds `serviceExecutor` for authorized due execution; terminal Audit result is `failure`. Manual Publish must not acquire an invented service identity. Main's standalone scheduler refuses startup without a qualified identity resolver. PoC wiring in #41 is not borrowed.

Legacy withdrawal/publication-end/management Audit data contains caller free-text `reason`; tests assert preservation. Withdrawal has no meaningful maximum; publication end's bound is essentially a u32 byte range. This track must neither silently rewrite those old rows/tests nor forward arbitrary free text. Independent review verified that a reference-only projection cannot preserve this evidence: withdrawal has no staged operation ID and Management ledgers keep digest/result, not reason. Reason-bearing variants are explicitly deferred/quarantined, never delivered/qualified, and their source evidence is ineligible for cleanup until a separately reviewed preservation contract or narrow user-approved amendment. Unknown fields/unsupported historical shapes are visible quarantine, never silently acknowledged as complete evidence.

## Search reuse map

Read-only WIP `outbox-delivery` has bounded claim/renew/fenced settle/reaper, deterministic backoff, unknown commit handling, capacity admission and lifecycle tests. Its concrete SQL owns `outbox_events`, envelope describes aggregate domain events, and its privilege tests explicitly prohibit Audit outbox mutation.

- Reuse now: failure model and test scenarios; no unaccepted source import or dependency.
- Adapt later: a common lease runner/transport interface after generic capability qualifies; Audit handler still validates and writes Audit Authority.
- Independent minimum now: audit-only source state namespace, source migration ledger, dispatcher and store port, preserving generic table/worker privileges untouched.
- Deferred: Search product audit policy and NO_RETENTION producer integration. Search specifies no query/body/remote evidence/default provider locator retention; Audit infrastructure must not create a way around that contract.
- Conflict trigger: shared domain outbox lifecycle ownership, automatic replacement of generic worker, or requiring WIP migrations. None is necessary for the chosen boundary.

## Independence, next stack and STOP

Audit evidence is distinct from DomainBusinessEvent, telemetry and Management Analytics. This track does not define Organization WorkItem/WorkContext/Workflow/Role/Assignment/Delegation or management KPI semantics; it adds no Personal AI code. Future integration receives versioned extension rules after Org Phase1/2 approval.

Proposed Draft stack: D-AUD1 design/selection/plan; A-AUD1 schema; A-AUD2 store/delivery; A-AUD3 Document integration/failure-security acceptance and future handoff. Keep each on its own exact qualified predecessor; no edits to #36–43/Search WIP/Organization specs; no merge/deploy/production migrations or credentials.

No core STOP identified. The legacy free-text producer subset is deferred/quarantined by the compatibility/privacy review; core infrastructure can proceed independently, but those variants cannot be called end-to-end qualified. Destructive source cleanup is also deferred without a qualified recoverable backup horizon. Shared cloud disk is about5GiB free; serialize Rust builds with other work and preserve truthful NOT RUN/FAIL receipts.

# Audit Infrastructure v1 — Management reason compatibility correction

Status: PROPOSED CORRECTION / FRESH SEMANTICS REVIEW REQUIRED. This amends A2 legacy-variant eligibility, not the approved Document contracts or the frozen Audit design/plan. It does not authorize historical backfill, producer edits, a reason-retrieval design, merge or deployment.

## Finding and exact authority

The frozen Document Management Basics design §6, line136 requires new T5–T8 operations to persist caller UUIDv7 operation ID, target, expected revision, trusted actor, **reason**, command digest and result in the durable ledger. Line150 describes allowed audit reason/changed-key/revision content; line261 requires bounded/minimized arbitrary inputs and prohibits Audit sampling. The separate trusted root-policy bootstrap is specified at line100; it is not a normal T8 command with a caller reason.

Source: `2026-09-28-document-management-basics-v0-design.md` and its design approval. Implementation evidence is the unchanged qualified baseline: `management_command.rs:43–92`, `management_digest.rs:179–202`, and `document-repository-postgres/src/access_policy.rs:311–344,484–499,635–649`.

All six ManagementCommand variants require a reason. Canonical command bytes contain it, but `insert_operation` writes only the digest and result fields, not the reason or original command bytes. A SHA256 digest or operation reference cannot retrieve the missing text. The five non-ACL changed mutations additionally retain reason in their Audit/Domain outbox payloads. Normal SetAccessPolicy does not. No-op outcomes deliberately emit no mutation event, so their missing ledger reason must not be masked by inventing an Audit event or by claiming outbox preservation.

## Cross-check of every reason-required Management command

| Command / Audit type | Actual changed-event evidence | Durable Management ledger | v1 disposition |
|---|---|---|---|
| UpdateDocumentMetadata / document.metadata.changed | `document_management.rs:268–274` stages reason | Digest/result only; required reason missing | Already fully deferred; protect original outbox evidence |
| MoveDocument / document.moved | `document_management.rs:435–444` stages reason | Same missing ledger field | Already fully deferred; protect original outbox evidence |
| CreateFolder / folder.created | `folder_management.rs:223–234` adds command reason | Same missing ledger field | Already fully deferred; protect original outbox evidence |
| RenameFolder / folder.renamed | Same common payload assembly adds reason | Same missing ledger field | Already fully deferred; protect original outbox evidence |
| MoveFolder / folder.moved | `folder_management.rs:465–474` stages reason | Same missing ledger field | Already fully deferred; protect original outbox evidence |
| SetAccessPolicy / access_policy.changed, non-bootstrap | `access_policy.rs:491–499` omits reason from the shared Audit/Domain payload | Same missing ledger field; no reconstructible reason evidence in these stores | **Deferred/quarantined**, even when no reason key is present |

For unchanged/replayed operations, preserve the existing no-new-mutation-event rule. This correction does not repair the broader ledger contract defect. Historical missing text must not be guessed, reconstructed from a digest, fabricated from the action, taken from personal memory/chat, or silently backfilled. A prospective additive producer/ledger repair may be considered later under the approved contract, with a separate preservation/access/retention review; it is not part of this A2 correction.

## Actual bootstrap-only eligible variant

The distinct `initialize_root_policy` producer accepts the configured trusted bootstrap actor and grants, with no caller-reason argument. Its actual payload/target are:

- `bootstrap=true`, required and never inferred from absence of reason
- Resource and metadata target type `Folder`
- Exact System Root Folder ID `00000000-0000-7000-8000-000000000001`
- A non-null policy ID, `policy_revision=1`, and positive `access_revision`
- Existing operation ID, actor, source, result, timestamp and field meanings unchanged

The old A2 synthetic fixture labeled bootstrap as a Document. That fixture was not an actual producer shape. Replace it with the Root Folder fixture and reject Document/arbitrary-Folder bootstrap claims. Normal ACL payloads with missing/false/non-true bootstrap remain reason-contract-unqualified; they are not upgraded because their payload happens to omit the required reason. Any explicit free-text reason still triggers the existing deferral, including an unexpected reason on a claimed bootstrap.

Catalog count stays20 existing event types:12 other eligible types, only the bootstrap variant of access_policy.changed eligible,7 fully deferred reason-bearing types, plus the normal ACL variant deferred. This is not full producer compatibility.

## Encoding, migration and review boundary

For a genuine Root Folder bootstrap, the envelope/canonical bytes remain unchanged. There is no new business-event taxonomy, no reason substitution, no source mutation and no event-ID migration. This narrows an over-permissive, undeployed Draft validator to the already-frozen semantics. Original frozen Audit design/plan and the approved Diff-node/correlation addendum stay byte-identical.

Required RED→GREEN: normal Document/Folder ACL payloads with absent/false bootstrap must defer in both legacy and envelope admission; JSON Schema must require actual bootstrap fields and reject non-root/nullable-policy/revision-zero variants; all five other reason-required Management types remain deferred even if their reason key is absent. Existing raw-JSON/privacy/identity/correlation tests must remain green. Obtain a fresh independent semantics review before updating Draft PR45. Its prior all-green CI receipts do not close this newly discovered compatibility gap.

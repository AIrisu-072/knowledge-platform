# Document Platform PoC Acceptance / Evaluation v0 — Evidence Report

Status: **PRELIMINARY DRAFT / VISUAL FAIL / C3 BLOCKED / OWNER ACCEPTANCE PENDING**.

The actual production-composition capture at H passed all 22 automated runtime
stages, including 10 browser cases and one persistence case. All 13 exported PNGs
were inspected: Japanese text appears as missing-glyph boxes throughout, so the
screens do not establish readable Human usability. Several views also omit their
header/target context because of scroll position. This report presents the failed
visual evidence now; it does not request final acceptance.

[Open the 13-image artifact](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36977734388/artifacts/11213789420)
while it remains available, until **2026-10-03 07:22:41 UTC**. The images are
synthetic and limited to the approved one-day retention. Automated runtime PASS,
file validation PASS and visual FAIL are separate results. Findings and remaining
work are in §6 and §11. Evidence observed through 2026-10-02 07:43 UTC.

## 1. Evidence subjects and closure order

| Symbol | Identity | Meaning |
|---|---|---|
| H0 | `706786970de25f74cb6f96d6a53c042d3da580dc`; tree `477613a2736f47ed1d8b5c73d74e999565b20af7` | Prior published PR43 subject; successful normal runtime, no visual capture |
| H | `31b75d81941027f3c00be0617fb26cd0f6a9e18c`; tree `43e393d821bc80b51f69879444570799014dd9ff` | Frozen acceptance subject; all three normal workflows succeeded before the one authorized capture activation |
| N | [CI36976219155 / job110740528535](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36976219155/job/110740528535) | Successful normal runtime at H; capture disabled; its own dataset and receipt below |
| V | [Capture CI36977734388 / job110745121719](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36977734388/job/110745121719), activated at `2026-10-02T07:16:48Z` | Actual real-composition capture on H; runtime PASS, artifact validated, visual FAIL; its own dataset below |
| R | This preliminary report; NOT PUBLISHED at preparation | Separate documentation-only commit based on H. Its exact identity is the containing commit/PR receipt; future hosted gates verify R and do not turn H images into R images |

The approved [E0 plan](../plans/2026-10-01-document-poc-acceptance-v0.md)
requires the repository report (E1), artifact IDs and matrix/provenance fields
(E3), and fresh exact-head hosted gates after documentation changes. Its global
same-run rule applies to Browser/API/stdio observations. This report preserves
that rule by recording the actual V bundle at H and identifying R separately.
H's screenshots will never be described as screenshots of R.

The repository has a corresponding recording convention: the
[DSI qualification report](document-semantic-inspection-v0-poc-report.md)
names its qualification execution head, while the
[DSI plan's Task9 final evidence](../plans/2026-09-20-document-semantic-inspection-v0-poc-qualification.md)
separately records later documentation-head gates. This is evidence-subject
separation, not a same-head exception.

Current order: retain H and its failed V evidence; independently review and publish
this preliminary documentation-only R; verify hosted gates on R. Visual blockers
require separately reviewed remediation and a distinct qualified source head/run.
A corrected capture must have its own receipt and pixel findings; this V never
becomes a visual PASS retrospectively. After complete evidence, report/status/runbook
and exact documentation-head gates, present the review as **READY FOR OWNER REVIEW**. The owner's explicit confirmation
is the final acceptance step, after required checks/review and no unresolved major
findings. Identify H/R and that decision in the PR evidence record without changing
H. CI success alone is not acceptance. No merge or deployment is authorized.

## 2. Capability and PR status

Repository/PR metadata below was read on 2026-10-02 around 06:33–06:36 UTC.
Every listed Document PR is OPEN, Draft and unmerged. Re-read before final publication.

C0 reconciliation: the later PR36 head differs from the R1 baseline `b578a9b…`
only in Active/C0 status, `.gitleaksignore` and the exact31 scanner exception note;
there is no GUI product delta in that C0 update. Its current gates were checked.
The C1 plan requires that reconciliation, while E0 requires C0–C3 statuses to stay
separate. PR36's own missing G9 receipt is not silently promoted by H's later
integrated GUI run. Conversely, no reviewed clause makes that historical label
an extra automatic C3 gate once H's required GUI path is actually qualified.
PR36 also lacks later GUI repairs present in H: CSP-safe schema validators, mixed-module
bundle resolution, sort transport, stable loading data and post-publication focus.
Its bounded backport/own-head G9 qualification remains separate proposed work;
H does not qualify those missing PR36 changes.
The original requested C0 closure remains an outstanding standalone deliverable;
this report does not declare the entire original request complete or authorize a successor.

| Capability | Current evidence | Status in this draft |
|---|---|---|
| C0 GUI closure | PR36 implementation and three hosted gates are green; its committed status explicitly leaves same-head G9 frontend/integrated acceptance pending | **G0–G8 / implementation complete; G9 final acceptance PENDING** |
| C1 composition/runtime | PR41 exact-head CI, DSI PoC and Sandbox all succeeded | **Hosted gates PASS**; detailed runtime/scheduler receipts must stay attributed to that head |
| C2 read-only Agent adapter | PR42 exact-head CI, DSI PoC and Sandbox all succeeded; actual runtime, focused MCP and scheduler jobs succeeded | **Hosted and actual runtime PASS at PR42 head** |
| C3 acceptance/evaluation | Normal H gates and V automated runtime passed; all 13 actual screenshots failed Japanese readability; R verification and owner confirmation remain | **VISUAL FAIL / BLOCKED; preliminary findings available, final acceptance not ready** |

| PR | Role / base branch | Exact current head |
|---|---|---|
| [36](https://github.com/AIrisu-072/knowledge-platform/pull/36) | GUI / `main` | `706307b980beb540db0759ad772ed4debd42c289` |
| [37](https://github.com/AIrisu-072/knowledge-platform/pull/37) | R1 design+plan / `feat/document-gui-integration-v0` | `dc9eb9bde55777934c100ce79c8c2b43ca8430eb` |
| [38](https://github.com/AIrisu-072/knowledge-platform/pull/38) | A1 design+plan / `design/document-poc-runtime-v0-r1` | `5a2b114964ddbe7d38dd6a5fe9b70fdad2cb56f1` |
| [39](https://github.com/AIrisu-072/knowledge-platform/pull/39) | E0 plan / `design/document-agent-tool-adapter-v0-a1` | `7d49a7bbde4d26bd072732c41cebe85a419fd4dd` |
| [41](https://github.com/AIrisu-072/knowledge-platform/pull/41) | R2 implementation / `design/document-poc-runtime-v0-r1` | `513529f5c256ee6e439dcdd16e746b417912ab2c` |
| [42](https://github.com/AIrisu-072/knowledge-platform/pull/42) | A2 implementation / `feat/document-poc-runtime-v0-r2` | `143ce4d5a07abbdca076f1f90c8e979234814be1` |
| [43](https://github.com/AIrisu-072/knowledge-platform/pull/43) | E1 harness/evidence / `feat/document-agent-tool-adapter-v0` | H above; normal gates PASS, V runtime PASS, visual FAIL |
| R | Preliminary evidence-only Draft / frozen H | This containing report commit; remote publication pending |

### Exact-head hosted receipts already observed

| Subject | CI | DSI PoC | Sandbox |
|---|---|---|---|
| PR36 `706307b9…` | [36946070147](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36946070147) SUCCESS | [36946070021](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36946070021) SUCCESS | [36946070032](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36946070032) SUCCESS |
| PR41 `513529f5…` | [36965695310](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36965695310) SUCCESS | [36965695308](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36965695308) SUCCESS | [36965695318](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36965695318) SUCCESS |
| PR42 `143ce4d5…` | [36972476928](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36972476928) SUCCESS | [36972476913](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36972476913) SUCCESS | [36972476940](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36972476940) SUCCESS |
| H `31b75d81…`, normal qualification | [36976219155](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36976219155) SUCCESS | [36976219179](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36976219179) SUCCESS | [36976219172](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36976219172) SUCCESS |
| Final R | PENDING | PENDING | PENDING |

PR37/38/39 design/plan CI runs
[36927672334](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36927672334),
[36927833055](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36927833055) and
[36928002797](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36928002797)
respectively succeeded; design-document CI is not runtime acceptance.
PR42's actual runtime [job110729172463](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36972476928/job/110729172463)
and separate scheduler [job110729172444](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36972476928/job/110729172444)
succeeded at its own head and dataset.

## 3. Prior normal H0 run, kept separate from V

[CI36972997256 runtime job110730749830](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36972997256/job/110730749830)
finished successfully at 2026-10-02T06:25:19Z. The bounded summary states
`acceptanceQualified=true`, clean H0, all22 stages passed, Agent phase `complete`,
nine completed check groups and `provenanceVerified=true`. Browser journey10/10
and persistence1/1 passed with no failures or skips. The visual gate was explicitly
disabled. These are normal-run results, not pixel-review or capture evidence.

Observed environment: Linux x64; Node24.21.0; pnpm12.4.1; Rust1.98.1;
Playwright1.63.0; PostgreSQL18.6. The emitted lock hashes were:

- Cargo: `dd7a87eb51bdd7be735b59b0fcf0961b0c500448d52c787b1d42352318bb0732`
- pnpm: `dffc848cd829592928ad81657d9b8c8bae57dae7ed83f4a17a2b395b45596559`

The summary also retained seven binary/bundle hashes and 12 web-asset hashes.
It did not emit actual ports, internal run UUID or fixture hash. Those fields are
not reconstructed, guessed or copied to V; the reporting gap prompted the bounded
pre-capture provenance correction. H0 remains a successful runtime observation
with that reporting limitation, not the final complete E3 receipt.

### Successful normal run N at the corrected H

Normal [runtime job110740528535](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36976219155/job/110740528535)
is SUCCESS and emits the actual bounded receipt below. Policy/security, focused
MCP and separate scheduler also succeeded. Aggregate checks were still running at
the 2026-10-02T07:07Z observation; all three normal workflows subsequently
completed SUCCESS before the single capture activation, independently re-read
through GitHub at approximately07:27Z. The capture workflow remains separate.
This is not V, and these values must not be copied into V's receipt.

| N field | Actual observed value |
|---|---|
| Run UUID | `e86799de-115f-4b86-b3a7-790bee270cb7` |
| Human / Agent ports | `38575` / `40279` |
| PostgreSQL / proxy ports | `32768` / `34305` |
| Owned database identity SHA256 | `1bf9b8d0dbecd37732ff798c1f1177f1cdb277eb1242936473dbf75797cb0918` |
| Storage identity SHA256 | `78c478cf47c73f205de47fe163dbd5cd227e18d7384307426c97be8cecace446` |
| Actual fixture hash | `4760715aa9faa6b56de7ffe75d7df46db201c456c086fe99426406d0a3ccc30b` |
| Restart identity verification | `true` |

The source verifies the actual owned container/run label/database OID and storage
device/inode before restart and after the existing persisted-state checks. Only
domain-separated hashes, the UUID, numeric ports and fixture hash are emitted;
raw IDs, paths, URLs and credentials are not published. These identity proofs add
to, rather than replace, the existing document/revision/file and Agent proofs.

## 4. Actual capture receipt V — runtime PASS, visual FAIL

The parent verified all three normal H workflows successful and an empty artifact
list, then applied the exact head label once at `2026-10-02T07:16:48Z`.
[Job110745121719](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36977734388/job/110745121719)
produced the bounded actual-run summary and exported the fixed images. Its
`acceptanceQualified=true` means the harness's automated runtime checks passed;
it does not mean visual or owner acceptance. No claim is made here about the
capture workflow's other still-unread aggregate results.

| Required identity | Actual V observation |
|---|---|
| H commit/tree; source clean | `31b75d81941027f3c00be0617fb26cd0f6a9e18c` / `43e393d821bc80b51f69879444570799014dd9ff`; `gitDirty=false` |
| Workflow/run/job | CI `36977734388`, job `110745121719`; first-attempt labeled-event gate |
| Platform/tool versions | Linux x64; Node24.21.0; pnpm12.4.1; Rust1.98.1; Playwright1.63.0; PostgreSQL18.6 |
| Actual synthetic fixture hash | `4760715aa9faa6b56de7ffe75d7df46db201c456c086fe99426406d0a3ccc30b` |
| Run UUID | `fd2632fc-a0b7-4258-8077-552c0040dac1` |
| Human / Agent ports | `41171` / `38331` |
| PostgreSQL / proxy ports | `32768` / `46687` |
| Run-bound owned database identity SHA256 | `84bb27a021484ab467b225e837b608cdab5178ffd5aa63f677eeca57eb19da0a` |
| Run-bound storage identity SHA256 | `56c230eed561f9519dd0be3d0d2903db8b16bfe5b5f25218237a5ab6b4734499` |
| Restart identity | `restartIdentityVerified=true`, alongside existing persisted-state assertions |
| Automated runtime | All 22 stages passed; browser10/10, persistence1/1; zero failed/skipped in each browser group |
| Agent | `status=passed`, `phase=complete`, nine completed groups, `provenanceVerified=true` |
| Artifact | `11213789420`, archive383160 bytes; SHA256 `2e1a49515687e4ad297124569b94067982affad247aaba3d44acdea20cda505b` |
| File validation | Exactly13 regular PNGs, CRC/dimensions validated; 12 at1440×900, one at1280×900; actual pixel review FAIL below |
| Created / expires | `2026-10-02T07:22:42Z` / `2026-10-03T07:22:41Z`, less than24 hours |
| Temporary local review | Temporary review copies only; deletion due no later than artifact expiry; cleanup not yet performed |

V's independently emitted source and artifact hashes:

| Item | SHA256 |
|---|---|
| cargo lock | `dd7a87eb51bdd7be735b59b0fcf0961b0c500448d52c787b1d42352318bb0732` |
| pnpm lock | `dffc848cd829592928ad81657d9b8c8bae57dae7ed83f4a17a2b395b45596559` |
| server | `c45bf3ba2ddd4752a59fd1a0b1586ad762b651d8d13d7fc0e86bfb64a9fc010d` |
| dsi | `25ddefcb08abca48cfd6fd875ae86a7887e81fe0664f711a8f46a9020f54c8f1` |
| diff | `0da11a2e7277cd37d899491d9576d0965cb2c22c806170e974cb790bd5967bda` |
| pdfium | `f728930966f503652b92acc89b9374a2eeca00ce42e26dccd3e4b5c5161b2d64` |
| mcp | `da47a7c7105f12fe11f52eacb7f66c99ecbff71fe6a1fe03e4678fa793195815` |
| mcpRuntime | `faf690ab06b523b3b58986e1a212ec0318d00836ce5459de84f24ca6d815a64d` |
| mcpConsistency | `bdc4d4a802ca4c4c7dc6a28b9ac8762eb436699e034d2e534e3d041c579f6851` |

The sorted 12 GUI asset SHA256 values emitted by V are:

- `123cd7e58aca0775a7b340aa18fdc5e0aed97e3f5eba1df8f9826bc7446a7b80`
- `1e7266cfe2810f20f53434fbe37cd3f24214ccbb21a6352a865982621662534f`
- `1fe5c154c4c8a1b1138ae4ba71436bbbd5c0f7caa1fca65da9626cc87d5339af`
- `388439386f84e9acb9d39471882e6643fab3de57f623fdf65ea310b9c64748a7`
- `541522236aafc5eb3e76d3329d3ce773c6fc5f465ca3f89826e48eaca8643a59`
- `5942e875702113122c8197a440df55e80f2bff28a5f974919b9c19e9c66695e8`
- `631c4ac033fb3ba1c2918929b6d2764b5207de95a8f59565f41f68386b58436c`
- `664d2d3224838a7f9fc13f7a6c9cf51b4cdb2f49c9f9fcb9c4788bcb21797f3c`
- `76d1f7d3f38eaed66860a2f4e20f879ad7d60502603a80d62f7d825a11da5161`
- `a7bcf39cee7192fd304bddb2f15a94713e9b18f41387361a6ebfa616d1cd93c5`
- `cb3b51417012353fb4e807696c13315f13d17511c67d9c44897c34a823c0f921`
- `ccef2aaf0e87a9925002493db66d9a25da2c8af818dc6c756fa8753a2a0ed4e9`

These observations belong to V; matching deterministic build/fixture hashes do
not make its run UUID or DB/storage identities interchangeable with N or H0.

The only approved shared files are the 13 synthetic PNGs. No raw logs, traces,
manifests, database URLs, physical paths or credentials enter the artifact.
The [bounded official-action ADR](../../decisions/2026-10-02-document-visual-upload-bounded-adoption.md)
and [visual procedure](../../operations/document-c3-visual-evidence.md) retain the
exact action pin, license decision, disclosed residual XML-response DoS risk,
one-day retention and a deliberate head-bound current-review activation. A corrective
retry, if reviewed and qualified, must use its own head/event; the old label must
not be removed/re-added or its event rerun automatically. No Library copy
or committed screenshot is permitted. PNG validation alone is not pixel review.

## 5. Acceptance matrix

H0 observations describe the earlier normal run. The H/V column is from V
only, except the explicitly separate scheduler gate. Browser PASS means automated
behavior; it does not override the visual FAIL. R observations remain separate.

| Requirement | H0 normal observation | Final H/V result |
|---|---|---|
| Built server, real PostgreSQL/FileSystemStorage, production sandboxed DSI/Diff | PASS | PASS, automated V |
| Explicit migrate, bootstrap and API seed replay | PASS | PASS, automated V |
| Fixed Human/Agent instances and authorized shared-state reads | PASS | PASS, automated V |
| Actual GUI folder/list/detail/revision/history/comparison/file journey | PASS | PASS, automated V |
| GUI create/publish; authoritative success and keyboard focus | PASS | PASS, automated V |
| Initial1.0, metadata minor, no-op invariant, new content major, withdrawal fallback | PASS | PASS, automated V |
| Exact Human/API/GUI/actual-stdio IDs, revisions, metadata and files | PASS | PASS, automated V |
| Same operation/payload recovery after lost mutation response, no duplicate | PASS | PASS, automated V |
| Stale OCC409 and fresh hidden-create404, retained file and restored201 retry | PASS | PASS, automated V |
| Human-only absence; distinct known-ID comparison denial and current-policy revoke | PASS | PASS, automated V |
| Real copied DSI unavailable503 and Diff unavailable500, no false success, restored workers | PASS | PASS, automated V |
| Common native-text PDF full comparison and annotated-PDF quality rejection | PASS | PASS, automated V |
| Health unavailable/recovery and safe diagnostics | PASS | PASS, automated V |
| SIGTERM ordinary drain; stalled stream remains draining until release | PASS | PASS, automated V |
| MCP outage/EOF/cancel/deadline boundaries | PASS; source-specific checks retain their own evidence class | PASS actual MCP/owned-process stages; bounded helper tests remain a separate evidence class |
| Restart with retained DB/storage, row/revision/file-hash equality and audit | PASS | PASS, automated V |
| Separate scheduler due/restart/exactly-once/revocation | Separate scheduler job | PASS in normal H CI36976219155; separate scheduler dataset, not a V browser/MCP observation |
| Actual visual usability at1440/1280; all13 pixel checkpoints | NOT RUN | **FAIL**, all13 Japanese text unreadable; framing findings below |
| Read-only Search inventory and Production Identity questions | Prepared below | Read-only inventory and questions recorded; no implementation |
| Final repository report and post-documentation R gates | NOT RUN | Preliminary report only; R hosted gates NOT RUN |
| Owner review of the presented images/report and explicit final acceptance | NOT GIVEN | NOT GIVEN |

DSI503 and Diff500 are their observed existing failure contracts; they are not
relabeled Unknown/Partial. No difference/unchanged, partial display or successful
mutation is fabricated on failure. Metadata and withdrawal use existing Human
Common API operations where the approved GUI has no mutation control; subsequent
GUI projection and actual MCP reads verify their outcomes.

## 6. Actual pixel review — FAIL and remediation blocked pending review

Method: inspect the actual V PNG pixels, alongside same-run DOM/keyboard assertions.
No external participant study, usability score, WCAG certification or performance
SLO is claimed. Scripted assertions and pixel observations are separate columns.

| File | Review concern | Actual pixel finding |
|---|---|---|
| `01-list-context-1440.png` | Folder/list context | FAIL: Japanese titles, labels and controls are boxes. Selected row and context structure are visible. List timestamp is7:21, versus16:21 in03. |
| `02-list-focus-return-1280.png` | Return focus and1280 layout | FAIL: Japanese copy is boxes. Selected row/focus indicator and context pane are visible; a long English title is clipped in its column. This static image alone cannot prove reduced motion. |
| `03-detail-overview-1440.png` | Detail hierarchy/current state | FAIL: Japanese copy is boxes. Version2, Revision2.0 and file metadata are visible; timestamp16:21 differs from01 for the same item. |
| `04-revision-version-1440.png` | Version, Revision and OCC distinction | FAIL: Numbered revisions, Version2 and selected-tab focus are visible, but unreadable Japanese explanations prevent a usability conclusion about the distinction. |
| `05-comparison-1440.png` | Comparison and disclosure | FAIL: Two-sided layout/change row is present, but Japanese compared content is boxes and cannot be read. |
| `06-version-file-selected-1440.png` | Create form and selected file | FAIL: `primary`,171B and form structure are visible. Japanese labels are boxes; right context is squeezed and filename/Version wrapping is awkward. |
| `07-publication-ready-1440.png` | Reviewed target before publish | FAIL: Scroll framing leaves checkbox/footer at the top and a largely blank image; target/header/context are absent. Japanese text is boxes. |
| `08-publication-confirm-focus-1440.png` | Confirmation and focus | FAIL: Centered dialog and blue confirm-button focus are visible, but Japanese dialog content is unreadable. Background remains scrolled/cropped. |
| `09-publication-success-1440.png` | Success and focus return | FAIL: Success strip and focused return button are visible, but Japanese feedback is unreadable and target/header context is offscreen. |
| `10-access-policy-effective-draft-1440.png` | Effective grants and draft | FAIL: Grant table, editable grants and English reason are visible, but Japanese actions are unreadable; header/binding context is clipped by scroll. |
| `11-occ-conflict-1440.png` | Conflict and retained input | FAIL: `stale.txt`, red alert and retry control are visible, but Japanese error text is unreadable and target/header context is offscreen. |
| `12-permission-denied-file-retained-1440.png` | Hidden404 and retained file | FAIL: `race-denied.txt`, red alert and retry control are visible, but Japanese error text is unreadable and upper form/header context is offscreen. |
| `13-permission-restored-retry-success-1440.png` | Restored201 success | FAIL: Success strip and return/action controls are visible, but Japanese feedback is unreadable and target/header context is offscreen. A button ends in an ellipsis; settled pending-state timing needs verification before classifying a product defect. |

Two independent assistant inspections examined all13 exported originals. This
is direct pixel inspection, not an external participant study or expert certification. Contrast/readability of
Japanese glyphs, comprehension of Version/Revision and actionable Japanese error
messages remain BLOCKED. DOM/keyboard tests passed their assertions; visible
focus/selected-state observations above are limited to the pixels actually shown.

Read-only diagnosis at H distinguishes evidence from hypotheses:

- **Font/render prerequisite, probable:** the font stack names Hiragino/Yu Gothic/Noto Sans JP then system sans; there is no bundled webfont. The runtime workflow installs Chromium but no explicit CJK font. Japanese DOM assertions passed while glyphs render as boxes, consistent with missing font coverage rather than demonstrated UTF-8 corruption. Hosted font inventory and Chromium's selected font have not yet been collected. No font has been adopted or installed by this report.
- **Framing, confirmed:** the capture helper takes `fullPage:false` at the current scroll position without a framing reset. Views07 and09–13 omit important context. A corrected capture needs to show both target and result without hiding the actual state or altering product content.
- **Timestamp formatting, confirmed source inconsistency:** Home uses `Intl.DateTimeFormat('ja-JP', {dateStyle:'medium', timeStyle:'short'})`; Detail explicitly adds `timeZone:'Asia/Tokyo'`. Playwright sets Japanese locale without a timezone override. The nine-hour image discrepancy is consistent with this difference. Choosing a browser timezone alone would not resolve the product formatter inconsistency; the approved intended display contract must govern any fix.
- **Capture timing, unqualified:** view13 shows a success strip while a button label ends in an ellipsis. Because the Japanese label is unreadable, this is not classified as a product defect. A corrected capture must verify the intended final pending state has settled without changing focus or business state for appearance.
- **Context width, observed but cause unqualified:** view06 squeezes the right pane and wraps the filename awkwardly. Recheck at1440/1280 with verified glyph coverage before assigning the cause to product layout versus font metrics.

Source locators: [font tokens](../../../apps/document-web/src/design-system/tokens.css),
[Home formatter](../../../apps/document-web/src/routes/DocumentHomePage.tsx),
[Detail formatter](../../../apps/document-web/src/routes/DocumentDetailPage.tsx),
[capture helper](../../../tools/document-poc-runtime/visual-evidence.mjs) and
[Playwright configuration](../../../apps/document-web/playwright.runtime.config.ts).
A missing or inadequate image is not a PASS inferred from another run.
No benchmark was performed; workflow/test timings are operational observations,
not interactive latency measurements or an approved performance threshold.

## 7. Agent interface and evaluation method

The adapter exposes these nine read-only generated-client tools via actual MCP
stdio. Inputs are bounded/strict; views, opaque cursors and current authorization
are retained. Comparison may create server-owned cache/audit records; that does not
give the Agent a mutation tool or direct DB/Application access.

| Tool | Main argument/discovery boundary |
|---|---|
| `document_get_root` | No arguments; authorized root metadata |
| `document_list_folder` | Folder UUID and bounded page/cursor |
| `document_list` | Explicit view, documented filters and bounded page/cursor |
| `document_get` | Document UUID and published/authoring view |
| `document_list_revisions` | Document UUID; human Major.Minor, not OCC |
| `document_get_history` | Document UUID; one authorized history page |
| `document_compare_versions` | Distinct content Version IDs, fixed `document-diff-v0`, explicit projection |
| `document_compare_revisions` | Distinct issued Revision IDs and projection; unavailable legacy stays unavailable |
| `document_list_files` | Document+Version UUIDs and explicit published/authoring/history purpose; metadata only |

Evaluation method: scripted real SDK client discovery/invocation over stdio
against the actual Agent process, plus source review of descriptions/schema/error
shapes. It is not an LLM tool-selection or user study. Descriptions distinguish
OCC, Version and human Revision; comparison descriptions explicitly warn that
Unknown/Partial/None/truncation do not prove unchanged. Exact hidden404 and bounded
problem shapes prevent existence/content disclosure. V completed all nine Agent groups with actual-stdio provenance and no reported
failure category. Argument/discovery assertions were scripted; no LLM tool-choice
or unscripted Agent usability study was run. Prior protocol fixtures remain unit
evidence and do not replace this real-server run.

### Observed text-alignment limitation

The unchanged text comparator removes a shared prefix/suffix and supports one
changed line, pure insertion or pure deletion. The earlier synthetic GUI fixture
replaced two remaining old lines with one new line. Against both seeded Versions,
the native unit-level probe returned Unknown/None with one `AmbiguousAlignment`
region and zero changes; expecting Full made both RED probes exit101. Correcting
only the fixture to retain the first two lines and edit line3 made both probes
exit0 with Full coverage, one change and no unverified region. Both GREEN probes
checked source hashes, byte counts and Diff/resource profiles.

The [C2 correction record](document-agent-tool-adapter-v0-status.md) preserves
that native RED/GREEN evidence and the checked-in fixture regression. The probe
used the existing compiled worker entrypoints without a Cargo rebuild or sandbox
override; it is comparator-level evidence, not by itself production-sandbox or
server/browser acceptance. H0 and V independently verified the supported single-line fixture in their
respective real compositions. No multi-line alignment capability was added. Unsupported
alignment must remain incomplete evidence and must never be interpreted as unchanged.

## 8. Composition, identity and operational limits

`document-server` composes PostgreSQL repository, FileSystemStorage, Application,
HTTP, production DSI/Diff runners and static PoC identity. Human and Agent are
separate processes of the same binary sharing that run's database/storage. Human
serves the built GUI and `/v1` same-origin; Agent serves no GUI.

Startup fixes Human to `poc/poc-human`, `poc-users`, HumanInteractive and Agent to
`poc/poc-agent`, `poc-agents`, Agent. Request headers/cookies/query/body do not
select identity. Default binding is loopback; nonloopback needs the explicit
override and is not a production security mechanism. Production mode and unknown
profiles fail closed. No production identity adapter was implemented.

`serve` checks exact schema compatibility without migration. Explicit `migrate`
and Human-only root bootstrap are documented in the
[runtime runbook](../../operations/document-poc-runtime-v0.md). Live/ready checks
return bounded status; readiness includes DB/schema/storage/workers/GUI checks.
SIGTERM drains current work; stalled streaming has no total drain deadline or
force-close guarantee. A lost mutation response requires the saved operation-ID
contract, not an assumed failure or a new operation ID.

Scheduler is a separate process. `service/scheduler` is executor audit attribution,
not a login principal or authorization grant; current requester rights are checked
again at execution/commit. Production TLS, DNS, AD/WIA/Kerberos/Entra, HA, backup,
deployment, Search/RAG and Agent write tools are outside this PoC acceptance.

## 9. Current Search inventory and next boundary

Read-only GitHub/source observations: 2026-10-02 06:28:59–06:31:37 UTC. These are
Search's own heads; they do not qualify a combined Document/Search runtime.

| State | Branch / exact head | Current hosted result |
|---|---|---|
| Qualified historical Phase D, Draft [PR33](https://github.com/AIrisu-072/knowledge-platform/pull/33) | `feat/search-discovery-platform-v0-d` / `4892ba5d2736b35bf95de25f834f016609d2e0d4` | CI36665497017, PoC36665497015, Sandbox36665497041 SUCCESS |
| Completion program, Draft [PR34](https://github.com/AIrisu-072/knowledge-platform/pull/34) | `feat/search-platform-completion-program` / `80a47960d025e4dfdea1eacade28b15d218725ff` | Program acceptance incomplete |
| Current held WIP, Draft [PR40](https://github.com/AIrisu-072/knowledge-platform/pull/40) | `feat/search-platform-cloud-continuation-20261001` / `a945fbd32145a3109e35cb9cb056cea052698138` | [CI36945866252](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36945866252) and [PoC36945866208](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36945866208) FAILURE; [Sandbox36945866263](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36945866263) SUCCESS |

Authoritative current-head paths:

- [Design](https://github.com/AIrisu-072/knowledge-platform/blob/a945fbd32145a3109e35cb9cb056cea052698138/docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design.md) and [approval](https://github.com/AIrisu-072/knowledge-platform/blob/a945fbd32145a3109e35cb9cb056cea052698138/docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design-approval.md)
- [Production plan](https://github.com/AIrisu-072/knowledge-platform/blob/a945fbd32145a3109e35cb9cb056cea052698138/docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-production-implementation.md) and [approval](https://github.com/AIrisu-072/knowledge-platform/blob/a945fbd32145a3109e35cb9cb056cea052698138/docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-production-implementation-approval.md)
- [Phase status](https://github.com/AIrisu-072/knowledge-platform/blob/a945fbd32145a3109e35cb9cb056cea052698138/docs/superpowers/execution/search-discovery-platform-v0-status.md), [program status](https://github.com/AIrisu-072/knowledge-platform/blob/a945fbd32145a3109e35cb9cb056cea052698138/docs/superpowers/execution/search-platform-completion-program-status.md), and [current held checkpoint](https://github.com/AIrisu-072/knowledge-platform/blob/a945fbd32145a3109e35cb9cb056cea052698138/docs/superpowers/programs/search-platform-completion/draft-publication-checkpoint-20261001.md)

Known continuation failures include missing `outbox_delivery::observe`, strict
Clippy large-enum diagnostics, eight wildcard path dependencies and yanked
`yoke-derive0.8.3`. Scanner self-test/history stages now pass their own checks;
that is not overall security PASS. PR33 success does not qualify PR40 WIP.
These are separately held Search work, not permission for E3 to repair it.

Phase D qualifies the Document Source/current-authorization/metadata boundary;
BodyRequired remains blocked rather than fabricated. Future P6 generic outbox
delivery, P5 trusted-identity HTTP and P7 runtime assembly contracts remain
incomplete. The next permitted action is to identify and independently review
the precise stopped operation and its authorized safe scope before resuming a
named Search task. E3 performs only this inventory; no source is copied or changed.

## 10. Production Identity questions only

Obtain answers and a separate design decision before implementing an adapter:

1. Are client devices AD-domain joined, Entra joined or hybrid, and which device classes must be supported?
2. Are Kerberos tickets available in the intended sessions, including biometric/Credential Provider login flows?
3. Which browsers and managed WIA/intranet policies are in use?
4. Where will the web service run: Linux/Windows, on-premises/cloud, and behind which reverse proxy?
5. Who owns the intended DNS name, TLS termination and realm/domain configuration?
6. Who will approve SPN ownership and service-account/keytab lifecycle? Do not send tickets, keytabs, passwords or secret values in chat.
7. Which user and department groups are authoritative, and how should they map to Document subjects/permissions?
8. Which identity attributes may be displayed, and how should unavailable/stale directory data appear?
9. How should Agent/service invocations be authenticated, attributed, revoked and audited independently of Human sessions?
10. Which operational owners will approve rotation, incident response and deployment prerequisites?

These are questions, not an AD/Kerberos/OIDC/SAML adapter selection or implementation.
Organization Client work is not started by this report.

## 11. Local evidence, remaining gates and final wording

Prior local candidate checks are recorded in [C3 Status](document-poc-acceptance-v0-status.md):
Node99, GUI44, MCP44, seed22, affected types/schema/actionlint and independent
source preservation review. They remain local harness/unit evidence and are not
substituted for final H/V or R hosted results. The separate provenance correction passed109 runtime-helper checks, runtime
TypeScript, actionlint and independent correctness/privacy review; its tree is H.
These checks qualify the harness change, while V supplies the actual receipt.

Outstanding C3 work: resolve the visual blockers under the approved scope, qualify
any distinct corrected source head, inspect its actual13-image capture, finish
report/status/runbook review and exact documentation-head gates, then present the
complete review for the owner's explicit confirmation. This preliminary report's
runtime results do not resolve the failed images. No recapture is performed by
this report, and no remedial source change is included.

Independent Audit limitation: the approved
[Management reason-retrieval clause](https://github.com/AIrisu-072/knowledge-platform/blob/d71753d46590bb4406a1c0b74894ab90a27a6c88/docs/superpowers/specs/2026-09-28-document-management-basics-v0-design.md#L136)
requires the T5–T8 ledger reason to be retrievable. The separate
[PR45 producer-compatibility investigation](https://github.com/AIrisu-072/knowledge-platform/pull/45)
remains open: current producer evidence retains digest/result only, and normal ACL
audit payload also omits reason. This is unresolved Audit/legacy-producer work;
Document history/persistence assertions in V do not qualify that missing reason
contract. No Audit fix or full Audit qualification is claimed here.

C0's separately stated G9 closure gap
remains an original-request deliverable, and known Search WIP remains separately
held; neither is silently promoted or substituted for final H's evidence.

After evidence, independent review and gates are complete, use
**READY FOR OWNER REVIEW: H qualified, report recorded at R, final acceptance
awaiting owner confirmation.** Only after that explicit confirmation may the
acceptance record say **C3 accepted for H; report recorded at R; required hosted
gates verified at R; owner confirmed the presented scope.** Name both exact
commits/runs and the actual decision. All PRs remain unmerged and undeployed.
No statement here authorizes production identity, Organization Client, Search
implementation, Agent writes, a merge or a deployment.

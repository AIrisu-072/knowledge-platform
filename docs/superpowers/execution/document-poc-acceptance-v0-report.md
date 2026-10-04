# Document Platform PoC Acceptance / Evaluation v0 — Evidence Report

Status: **CORRECTED RUNTIME PASS / VISUAL GO WITH DISCLOSED LIMITS / FINAL REPORT GATES PENDING / OWNER ACCEPTANCE PENDING**.

The corrected source H2 passed all three normal hosted workflows. Its capture
V2 passed all22 automated runtime stages,11 browser cases and one persistence
case. All13 original PNGs were inspected: Japanese is readable, earlier lost
context and upload-pending framing are resolved, and zones/offsets are explicit.
The images still have documented limitations:01 shows selected-detail loading,
02 truncates a long list title,08 includes a viewport-fixed backdrop in a full-page
capture,09 clears the authoring target after publication, and11/12 place trace
IDs tightly against reload buttons. No flawless or all13-settled claim is made.

[Review the corrected13 images](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36989549579/artifacts/11218564738)
before **2026-10-03 09:29:52 UTC**. These synthetic images retain the approved
one-day sharing boundary. Historical V1 visual FAIL remains below. Independent factual/privacy report review is GO; applicable report-R2 hosted
gates and owner confirmation remain pending.
Actual image/artifact observations and capture aggregate gates are recorded
through2026-10-02 09:41 UTC.

## 1. Evidence subjects and closure order

| Symbol | Identity | Meaning |
|---|---|---|
| H0 | `706786970de25f74cb6f96d6a53c042d3da580dc`; tree `477613a2736f47ed1d8b5c73d74e999565b20af7` | Historical successful normal runtime with an incomplete original port/run-identity receipt; no capture |
| H1 | `31b75d81941027f3c00be0617fb26cd0f6a9e18c`; tree `43e393d821bc80b51f69879444570799014dd9ff` | Historical source with all three normal workflows successful |
| N1 | [CI36976219155 / job110740528535](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36976219155/job/110740528535) | H1 normal non-capture run; its own dataset |
| V1 | [CI36977734388 / job110745121719](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36977734388/job/110745121719); artifact11213789420 | H1 actual capture: automated runtime PASS, file validation PASS, visual FAIL |
| Report R1 | [Draft PR46](https://github.com/AIrisu-072/knowledge-platform/pull/46) `f49866f0fef88d2735db062b83c3ad686e9097b6`; tree `12d156471a53ddbd5eae0f26c007f57b51df4161` | Published preliminary FAIL report; its own applicable CI succeeded |
| H2 | `6103e4d4e3bb0d45ba03e1d2935492de7f11394a`; tree `f2e13eee0d7e1bfa71952c1da52a72cecb65fc9e` | Corrected source; independently reviewed local4b15a492 is tree-equivalent, not its hosted commit |
| N2 | [CI36987407999 / job110775471680](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36987407999/job/110775471680) | H2 actual normal non-capture proof; its own run-bound receipt |
| V2 | [CI36989549579](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36989549579), started2026-10-02T09:24:02Z | Actual H2 capture; runtime PASS, artifact11218564738 validated, pixels reviewed with disclosed limits |
| Report R2 | This containing documentation commit; exact SHA/tree in its publication receipt | Evidence-only integration of report R1 and H2; own exact-head gates pending at preparation |

The approved [E0 plan](../plans/2026-10-01-document-poc-acceptance-v0.md)
requires the repository report (E1), artifact IDs and matrix/provenance fields
(E3), and fresh exact-head hosted gates after documentation changes. Its global
same-run rule applies to Browser/API/stdio observations. This report preserves
that rule by recording V1 at H1 and V2 at H2, with separate run identities
and report R1/R2 documentation subjects. A source head's screenshots are never
described as screenshots of its later documentation head.

The repository has a corresponding recording convention: the
[DSI qualification report](document-semantic-inspection-v0-poc-report.md)
names its qualification execution head, while the
[DSI plan's Task9 final evidence](../plans/2026-09-20-document-semantic-inspection-v0-poc-qualification.md)
separately records later documentation-head gates. This is evidence-subject
separation, not a same-head exception.

Closure order keeps H2 frozen. Report R2 has exact parents report R1
`f49866f0fef88d2735db062b83c3ad686e9097b6` and H2
`6103e4d4e3bb0d45ba03e1d2935492de7f11394a`, with a tree differing from H2 only
at the five evidence/status/runbook paths. Both are ancestors; this preserves
PR46 history and permits a fast-forward update. It is local Git ancestry
integration, not a GitHub PR merge, closure or deployment.

V2 supplied its own runtime, artifact and actual pixel evidence; independent
report review returned GO. Applicable exact report-R2 hosted gates follow. The containing
commit cannot include its own computed hash or future CI result, so the exact
report-R2 publication/check receipt is recorded on the Draft PR after checks,
without changing H2 or inventing source equality. Report-R2 runtime observations
are never copied into V2. After those requirements pass, present **READY FOR OWNER
REVIEW**. The owner's explicit confirmation remains final acceptance; CI alone
does not grant it. Historical V1 remains visual FAIL.

## 2. Capability and PR status

Read-only repository/PR metadata was refreshed at2026-10-02 09:27 UTC.
All eight listed Document PRs remain OPEN, Draft and unmerged at the exact heads
below. Historical workflow dates still identify their actual earlier runs.

C0 reconciliation: the later PR36 head differs from the R1 baseline `b578a9b…`
only in Active/C0 status, `.gitleaksignore` and the exact31 scanner exception note;
there is no GUI product delta in that C0 update. Its current gates were checked.
The C1 plan requires that reconciliation, while E0 requires C0–C3 statuses to stay
separate. PR36's own missing G9 receipt is not promoted by a later integrated GUI
run on H1 or H2. That original C0 deliverable remains open; it is not an extra
automatic C3 gate substituted for the required GUI qualification on the C3 head.
PR36 also lacks later GUI repairs present in H1: CSP-safe schema validators, mixed-module
bundle resolution, sort transport, stable loading data and post-publication focus.
Its bounded backport/own-head G9 qualification remains separate proposed work;
H1 does not qualify those missing PR36 changes.
The original requested C0 closure remains an outstanding standalone deliverable;
this report does not declare the entire original request complete or authorize a successor.

| Capability | Current evidence | Status in this draft |
|---|---|---|
| C0 GUI closure | PR36 implementation and three hosted gates are green; its committed status explicitly leaves same-head G9 frontend/integrated acceptance pending | **G0–G8 / implementation complete; G9 final acceptance PENDING** |
| C1 composition/runtime | PR41 exact-head CI, DSI PoC and Sandbox all succeeded | **Hosted gates PASS**; detailed runtime/scheduler receipts must stay attributed to that head |
| C2 read-only Agent adapter | PR42 exact-head CI, DSI PoC and Sandbox all succeeded; actual runtime, focused MCP and scheduler jobs succeeded | **Hosted and actual runtime PASS at PR42 head** |
| C3 acceptance/evaluation | H2 normal workflows and N2/V2 actual runtime passed; V1 pixels failed; V2 pixels reviewed with disclosed limits | **FINAL REPORT REVIEW/GATES PENDING / OWNER ACCEPTANCE PENDING** |

| PR | Role / base branch | Exact current head |
|---|---|---|
| [36](https://github.com/AIrisu-072/knowledge-platform/pull/36) | GUI / `main` | `706307b980beb540db0759ad772ed4debd42c289` |
| [37](https://github.com/AIrisu-072/knowledge-platform/pull/37) | R1 design+plan / `feat/document-gui-integration-v0` | `dc9eb9bde55777934c100ce79c8c2b43ca8430eb` |
| [38](https://github.com/AIrisu-072/knowledge-platform/pull/38) | A1 design+plan / `design/document-poc-runtime-v0-r1` | `5a2b114964ddbe7d38dd6a5fe9b70fdad2cb56f1` |
| [39](https://github.com/AIrisu-072/knowledge-platform/pull/39) | E0 plan / `design/document-agent-tool-adapter-v0-a1` | `7d49a7bbde4d26bd072732c41cebe85a419fd4dd` |
| [41](https://github.com/AIrisu-072/knowledge-platform/pull/41) | R2 implementation / `design/document-poc-runtime-v0-r1` | `513529f5c256ee6e439dcdd16e746b417912ab2c` |
| [42](https://github.com/AIrisu-072/knowledge-platform/pull/42) | A2 implementation / `feat/document-poc-runtime-v0-r2` | `143ce4d5a07abbdca076f1f90c8e979234814be1` |
| [43](https://github.com/AIrisu-072/knowledge-platform/pull/43) | E1 harness/evidence / `feat/document-agent-tool-adapter-v0` | H2 above; normal gates PASS; V2 runtime PASS and pixel findings below |
| [46](https://github.com/AIrisu-072/knowledge-platform/pull/46) | Evidence-only Draft / PR43 source branch | Published report R1 `f49866f0…`; report R2 preparation pending final evidence |

### Exact-head hosted receipts already observed

| Subject | CI | DSI PoC | Sandbox |
|---|---|---|---|
| PR36 `706307b9…` | [36946070147](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36946070147) SUCCESS | [36946070021](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36946070021) SUCCESS | [36946070032](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36946070032) SUCCESS |
| PR41 `513529f5…` | [36965695310](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36965695310) SUCCESS | [36965695308](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36965695308) SUCCESS | [36965695318](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36965695318) SUCCESS |
| PR42 `143ce4d5…` | [36972476928](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36972476928) SUCCESS | [36972476913](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36972476913) SUCCESS | [36972476940](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36972476940) SUCCESS |
| H1 `31b75d81…`, normal qualification | [36976219155](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36976219155) SUCCESS | [36976219179](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36976219179) SUCCESS | [36976219172](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36976219172) SUCCESS |
| Preliminary report R1 `f49866f0…` | [36981136324](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36981136324) SUCCESS | NOT TRIGGERED: report-only path | NOT TRIGGERED: report-only path |
| H2 `6103e4d4…`, normal qualification | [36987407999](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36987407999) SUCCESS | [36987408029](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36987408029) SUCCESS | [36987408087](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36987408087) SUCCESS |
| Final report R2 | NOT RUN at preparation | Applicable through Active; NOT RUN | Applicable through Active; NOT RUN |

H2 normal [required-check110780804804](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36987407999/job/110780804804)
succeeded before the deliberate corrected capture. Report R1's sole report path
matches neither DSI workflow filter. Report R2 changes Active, an explicit path
in both filters, so all three workflows apply. CI has no PR path filter and its
required-check requires all ten predecessor job definitions, including actual
runtime, focused MCP and the separate scheduler. Documentation scope does not
waive those required paths. Optional DSI PoC macOS jobs have their own Draft
conditions; CI's required macOS parity jobs are separate.

PR37/38/39 design/plan CI runs
[36927672334](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36927672334),
[36927833055](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36927833055) and
[36928002797](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36928002797)
respectively succeeded; design-document CI is not runtime acceptance.
PR42's actual runtime [job110729172463](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36972476928/job/110729172463)
and separate scheduler [job110729172444](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36972476928/job/110729172444)
succeeded at its own head and dataset.

## 3. Prior normal H0 run, kept separate from V1

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
not reconstructed, guessed or copied to V1; the reporting gap prompted the bounded
pre-capture provenance correction. H0 remains a successful runtime observation
with that reporting limitation, not the final complete E3 receipt.

### Successful normal run N1 at the corrected H1

Normal [runtime job110740528535](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36976219155/job/110740528535)
is SUCCESS and emits the actual bounded receipt below. Policy/security, focused
MCP and separate scheduler also succeeded. Aggregate checks were still running at
the 2026-10-02T07:07Z observation; all three normal workflows subsequently
completed SUCCESS before the single capture activation, independently re-read
through GitHub at approximately07:27Z. The capture workflow remains separate.
This is not V1, and these values must not be copied into V1's receipt.

| N1 field | Actual observed value |
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

### Corrected normal runtime N2 at H2

The bounded receipt reports status passed, acceptanceQualified=true and gitDirty=false at exact H2. All22 stages passed. Journey11/11 and persistence1/1 passed with zero failed/skipped; Agent phase complete with nine groups and provenanceVerified=true. These are actual production-composition/browser/stdio results, without a visual acceptance claim.

A passed original journey emitted fontSelection=kosugi-regular-japanese-heading-body. This is actual Chromium CDP selection for the existing Japanese heading/body nodes, beyond static font coverage. N2 alone does not establish the regular-only face hierarchy or remaining symbol appearance; actual V2 pixel findings are in §6.

A passed timestamp-layout.spec.ts emitted timestampLayout=long-iana-both-folds-1280-1440. Its six measured Chromium cases use the real product formatter and built-app CSS in isolated read-only display contexts: the long America/North_Dakota/New_Salem label and both repeated New York fall-back instants at1280/1440. It proves bounded timestamp fragments/row separation under those cases. Only cloned timestamp text is substituted; this is display geometry evidence, not backend timestamp equality or persistence evidence. It emits no screenshots, trace, video or test attachments.

The normal job separately recorded the visual gate step successful, KP_POC_VISUAL_REVIEW_ENABLED:false at2026-10-02T09:02:18.0607772Z, and the approved synthetic upload step SKIPPED. These facts come from the observed job step/log receipt, not from a field in the bounded runtime JSON.

| N2 field | Actual observed value |
|---|---|
| Exact head / clean source | `6103e4d4e3bb0d45ba03e1d2935492de7f11394a` / `gitDirty=false` |
| Run UUID | `86636377-5d05-41d2-8892-ce9c1a86f95c` |
| Human / Agent / PostgreSQL / proxy ports | `39853` / `35007` / `32768` / `46431` |
| Owned database identity SHA256 | `d979d2e0c52b86de795c201b4915cd107a30aa187fdc1f48b6e72aa22626ddc0` |
| Storage identity SHA256 | `625f4cd7a9e48f2e030acdaae48bc0210c72d9a21433d88dfcd237cd2f3c0423` |
| Actual fixture manifest hash | `4760715aa9faa6b56de7ffe75d7df46db201c456c086fe99426406d0a3ccc30b` |
| Ownership / restart identity | `harness-owned` / `restartIdentityVerified=true` |
| Platform / tools | Linux x64; Node24.21.0; pnpm12.4.1; Rust1.98.1; Playwright1.63.0; PostgreSQL18.6 |

The actual UUID and database/storage digests differ from N1 and V1. The deterministic fixture hash repeats by design and never establishes dataset identity by itself. Existing exact row/revision/file state and restart assertions remain mandatory. No ports or identities are copied between N2 and V2.

N2 source/binary provenance (Cargo and pnpm lock hashes independently match the reviewed local-equivalent tree):

| Item | SHA256 |
|---|---|
| cargo | `dd7a87eb51bdd7be735b59b0fcf0961b0c500448d52c787b1d42352318bb0732` |
| pnpm | `dffc848cd829592928ad81657d9b8c8bae57dae7ed83f4a17a2b395b45596559` |
| server | `c45bf3ba2ddd4752a59fd1a0b1586ad762b651d8d13d7fc0e86bfb64a9fc010d` |
| dsi | `25ddefcb08abca48cfd6fd875ae86a7887e81fe0664f711a8f46a9020f54c8f1` |
| diff | `0da11a2e7277cd37d899491d9576d0965cb2c22c806170e974cb790bd5967bda` |
| pdfium | `f728930966f503652b92acc89b9374a2eeca00ce42e26dccd3e4b5c5161b2d64` |
| mcp | `da47a7c7105f12fe11f52eacb7f66c99ecbff71fe6a1fe03e4678fa793195815` |
| mcpRuntime | `faf690ab06b523b3b58986e1a212ec0318d00836ce5459de84f24ca6d815a64d` |
| mcpConsistency | `bdc4d4a802ca4c4c7dc6a28b9ac8762eb436699e034d2e534e3d041c579f6851` |

N2 sorted GUI asset SHA256 values:

- `123cd7e58aca0775a7b340aa18fdc5e0aed97e3f5eba1df8f9826bc7446a7b80`
- `1e7266cfe2810f20f53434fbe37cd3f24214ccbb21a6352a865982621662534f`
- `1f081d0eeb867126ac383df44748adf05c5cb2e6593f2dddec18512535c2349c`
- `1f514931f200d04c75782342d41fdcc1b51db0f9144966ebb788d348d12dd853`
- `481afe4eaee5eb79b7dc9ba30215b79b3abf13ac82c1fb2ef461670266e7503a`
- `4be474c11b1635adca6d998fe3ce2a26e7ead7d464537af282790f1973cc6b44`
- `664d2d3224838a7f9fc13f7a6c9cf51b4cdb2f49c9f9fcb9c4788bcb21797f3c`
- `a7bcf39cee7192fd304bddb2f15a94713e9b18f41387361a6ebfa616d1cd93c5`
- `b471f5f674489c1efe8adeeb94a701004ca65384707b8820c51835bd4941eb1e`
- `cb3b51417012353fb4e807696c13315f13d17511c67d9c44897c34a823c0f921`
- `ed5f32f318f0a75c865d77d429d3eea0c823e11a28b3dc84186f1bc5103f9a0c`
- `f0622796ea0d8600b5ac494d238538bf1f2b4a43f938bbbe9d7f28e858a15955`

The bounded summary SHA256 is `6802dfcb01328360f813e140b8d8929f8fd793a622c3e4dd54a4a1e5437de5b6`. Source-bound startup counters include intentional negative API paths; a passed test does not imply zero negative HTTP responses or console messages. No raw logs, paths, credentials or manifests need to be included in the visual artifact.


## 4. Historical capture receipt V1 — runtime PASS, visual FAIL

The parent verified all three normal H1 workflows successful and an empty artifact
list, then applied the exact head label once at `2026-10-02T07:16:48Z`.
[Job110745121719](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36977734388/job/110745121719)
produced the bounded actual-run summary and exported the fixed images. Its
`acceptanceQualified=true` means the harness's automated runtime checks passed;
it does not mean visual or owner acceptance. No claim is made here about the
capture workflow's other still-unread aggregate results.

| Required identity | Actual V1 observation |
|---|---|
| H1 commit/tree; source clean | `31b75d81941027f3c00be0617fb26cd0f6a9e18c` / `43e393d821bc80b51f69879444570799014dd9ff`; `gitDirty=false` |
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

V1's independently emitted source and artifact hashes:

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

The sorted 12 GUI asset SHA256 values emitted by V1 are:

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

These observations belong to V1; matching deterministic build/fixture hashes do
not make its run UUID or DB/storage identities interchangeable with N1 or H0.

The only approved shared files are the 13 synthetic PNGs. No raw logs, traces,
manifests, database URLs, physical paths or credentials enter the artifact.
The [bounded official-action ADR](../../decisions/2026-10-02-document-visual-upload-bounded-adoption.md)
and [visual procedure](../../operations/document-c3-visual-evidence.md) retain the
exact action pin, license decision, disclosed residual XML-response DoS risk,
one-day retention and a deliberate head-bound current-review activation. A corrective
retry, if reviewed and qualified, must use its own head/event; the old label must
not be removed/re-added or its event rerun automatically. No Library copy
or committed screenshot is permitted. PNG validation alone is not pixel review.

### Corrected capture V2 — automated PASS; actual pixels reviewed

After all normal H2 gates passed, its exact head label was added once at
2026-10-02T09:24:00Z. Capture CI36989549579 started09:24:02Z. The old label/event
was not removed, re-added or rerun. The actual runtime job and fixed upload
succeeded. The complete capture workflow subsequently finished SUCCESS at the same H2;
all12 returned jobs and [required-check110787558098](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36989549579/job/110787558098)
succeeded, verified2026-10-02 09:41 UTC. These capture-run gates remain separately
attributed from normal N2 and future report-R2 gates.

[Open the corrected13-image artifact](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36989549579/artifacts/11218564738)
before its actual expiry **2026-10-03 09:29:52 UTC**. Actual pixels were inspected
at original resolution, not generated, repaired or cropped for this report.
The per-view findings and explicit limitations are in §6.

| Required V2 evidence | Actual observation |
|---|---|
| Exact source / tree / clean | `6103e4d4e3bb0d45ba03e1d2935492de7f11394a` / `f2e13eee0d7e1bfa71952c1da52a72cecb65fc9e` / `gitDirty=false` |
| Workflow / job | [CI36989549579 / job110782297061](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36989549579/job/110782297061) SUCCESS |
| Automated runtime | All22 stages PASS; journey11/11 and persistence1/1, zero failed/skipped |
| Agent | Complete, nine check groups, provenanceVerified=true |
| Actual font / layout markers | `kosugi-regular-japanese-heading-body` / `long-iana-both-folds-1280-1440`, both on passed tests |
| Run UUID | `6c55cb67-0eaa-40a0-878d-fbc66b19a451` |
| Human / Agent / PostgreSQL / proxy ports | `42807` / `34811` / `32768` / `40233` |
| Owned database identity SHA256 | `ea4e2eed997e586647374c13e2b780bbaf73b16e40471bfbabc5f95136fded99` |
| Storage identity SHA256 | `5f39a4387264bb2284f9ed2f38cc512d7b576aa137f10259cfe0ee91783abf15` |
| Actual fixture manifest hash | `4760715aa9faa6b56de7ffe75d7df46db201c456c086fe99426406d0a3ccc30b` |
| Ownership / restart identity | `harness-owned` / `restartIdentityVerified=true` |
| Platform / tools | Linux x64; Node24.21.0; pnpm12.4.1; Rust1.98.1; Playwright1.63.0; PostgreSQL18.6 |
| Artifact / archive | `11218564738`;1,180,319bytes; SHA256 `028f31be34b9c28baa34718568d69dcd7cd2c520b3d8e9f81f19b2619ff67454` |
| Created / expires | `2026-10-02T09:29:53Z` / `2026-10-03T09:29:52Z`; actual API metadata,23h59m59s |
| Validation | Exact13 allowlisted regular PNGs; ZIP and PNG chunk CRC, no ancillary metadata;01–05 height900;06–13 height1304–1468; each below8MiB |
| Temporary local review | Review copies only, deletion due by actual expiry; cleanup not yet performed |

V2 independently emitted the following lock/binary hashes. All12 V2 GUI asset
hashes also exactly equal the12 N2 values listed above; this equality was checked
against both actual summaries. Matching deterministic artifacts do not make the
separate UUIDs or database/storage identities interchangeable.

| V2 item | SHA256 |
|---|---|
| cargo | `dd7a87eb51bdd7be735b59b0fcf0961b0c500448d52c787b1d42352318bb0732` |
| pnpm | `dffc848cd829592928ad81657d9b8c8bae57dae7ed83f4a17a2b395b45596559` |
| server | `c45bf3ba2ddd4752a59fd1a0b1586ad762b651d8d13d7fc0e86bfb64a9fc010d` |
| dsi | `25ddefcb08abca48cfd6fd875ae86a7887e81fe0664f711a8f46a9020f54c8f1` |
| diff | `0da11a2e7277cd37d899491d9576d0965cb2c22c806170e974cb790bd5967bda` |
| pdfium | `f728930966f503652b92acc89b9374a2eeca00ce42e26dccd3e4b5c5161b2d64` |
| mcp | `da47a7c7105f12fe11f52eacb7f66c99ecbff71fe6a1fe03e4678fa793195815` |
| mcpRuntime | `faf690ab06b523b3b58986e1a212ec0318d00836ce5459de84f24ca6d815a64d` |
| mcpConsistency | `bdc4d4a802ca4c4c7dc6a28b9ac8762eb436699e034d2e534e3d041c579f6851` |

Actual exported file provenance:

| Filename | Dimensions | Bytes | SHA256 |
|---|---|---|---|
| `01-list-context-1440.png` | 1440×900 | 107028 | `20e179d07a1e4601d1eb6e9d00cfb9d23b538228b5b03a11f572d0c2a0f4186e` |
| `02-list-focus-return-1280.png` | 1280×900 | 114329 | `30933e63945229f2f31cbb6e4627a4bbff51ac9ab9fe99b994732f21ff4d6786` |
| `03-detail-overview-1440.png` | 1440×900 | 108181 | `2e3fcf1c4d8a959978e7229ace778bc067a0169c51fd63519c2e4392bd8d55c0` |
| `04-revision-version-1440.png` | 1440×900 | 118230 | `2bac4cd7bec26126d5a52a5dc746f22eb0ba674a4ffef01eb6dee06630becc9e` |
| `05-comparison-1440.png` | 1440×900 | 62616 | `6f0cb80ed1dc85dda78dca4a1e9ffc1fb8226138899f643392e586105b5c3f55` |
| `06-version-file-selected-1440.png` | 1440×1441 | 66330 | `26285e09fdea5e2dea063aa7397e7542b13d7201da639a98c25a30af1b4378d4` |
| `07-publication-ready-1440.png` | 1440×1468 | 67762 | `bd6a2499adfe9037266553fce019370b1cc8a2c79d5f4b93103aadb763cbfda2` |
| `08-publication-confirm-focus-1440.png` | 1440×1468 | 79987 | `4df229ca901f9e2821e60f8771234289c05387fa22f5cdee83f22514a64d17e5` |
| `09-publication-success-1440.png` | 1440×1304 | 77741 | `8c311c946f0223190afc9239fa1c2e7e48d5eb4bf95319fc663fb7469e53475a` |
| `10-access-policy-effective-draft-1440.png` | 1440×1441 | 131053 | `7061b4e1403d973b416737ab4686cb2351f9c3374f56cf0d37582df2865786a1` |
| `11-occ-conflict-1440.png` | 1440×1412 | 85455 | `25156483c9001f4459bfddea1e0b8ebd334ca9bc47abc647fa9b93168b93cd76` |
| `12-permission-denied-file-retained-1440.png` | 1440×1466 | 84481 | `ca1835e062ae8bb0cd663fc3d2a4d6aaadb60559ff44d683dfddf3be85b12543` |
| `13-permission-restored-retry-success-1440.png` | 1440×1455 | 75082 | `16af6ece02cfba88b0e2b4e73053ad7164b9bec371b2820ed8311e43f13c980f` |

The file validator and actual pixel review are separate.01 contains an explicitly
labeled pending detail query; its ready state is not proven by that image.02
independently shows the ready selected context in V2. This limitation and the
full-page/modal and completion-state observations remain visible in §6.

## 5. Acceptance matrix

H0 observations describe the earlier normal run. The historical H1/V1 column is from V1
only, except the explicitly separate scheduler gate. Browser PASS means automated
behavior; it does not override the visual FAIL. R1 observations remain separate.

| Requirement | H0 normal observation | Historical H1/V1 result |
|---|---|---|
| Built server, real PostgreSQL/FileSystemStorage, production sandboxed DSI/Diff | PASS | PASS, automated V1 |
| Explicit migrate, bootstrap and API seed replay | PASS | PASS, automated V1 |
| Fixed Human/Agent instances and authorized shared-state reads | PASS | PASS, automated V1 |
| Actual GUI folder/list/detail/revision/history/comparison/file journey | PASS | PASS, automated V1 |
| GUI create/publish; authoritative success and keyboard focus | PASS | PASS, automated V1 |
| Initial1.0, metadata minor, no-op invariant, new content major, withdrawal fallback | PASS | PASS, automated V1 |
| Exact Human/API/GUI/actual-stdio IDs, revisions, metadata and files | PASS | PASS, automated V1 |
| Same operation/payload recovery after lost mutation response, no duplicate | PASS | PASS, automated V1 |
| Stale OCC409 and fresh hidden-create404, retained file and restored201 retry | PASS | PASS, automated V1 |
| Human-only absence; distinct known-ID comparison denial and current-policy revoke | PASS | PASS, automated V1 |
| Real copied DSI unavailable503 and Diff unavailable500, no false success, restored workers | PASS | PASS, automated V1 |
| Common native-text PDF full comparison and annotated-PDF quality rejection | PASS | PASS, automated V1 |
| Health unavailable/recovery and safe diagnostics | PASS | PASS, automated V1 |
| SIGTERM ordinary drain; stalled stream remains draining until release | PASS | PASS, automated V1 |
| MCP outage/EOF/cancel/deadline boundaries | PASS; source-specific checks retain their own evidence class | PASS actual MCP/owned-process stages; bounded helper tests remain a separate evidence class |
| Restart with retained DB/storage, row/revision/file-hash equality and audit | PASS | PASS, automated V1 |
| Separate scheduler due/restart/exactly-once/revocation | Separate scheduler job | PASS in normal H1 CI36976219155; separate scheduler dataset, not a V1 browser/MCP observation |
| Actual visual usability at1440/1280; all13 pixel checkpoints | NOT RUN | **FAIL**, all13 Japanese text unreadable; framing findings below |
| Read-only Search inventory and Production Identity questions | Prepared below | Read-only inventory and questions recorded; no implementation |
| Repository report and post-documentation gates | NOT RUN at H0 | Preliminary report R1 CI subsequently PASS; final report R2 gates pending |
| Owner review of the presented images/report and explicit final acceptance | NOT GIVEN | NOT GIVEN |

DSI503 and Diff500 are their observed existing failure contracts; they are not
relabeled Unknown/Partial. No difference/unchanged, partial display or successful
mutation is fabricated on failure. Metadata and withdrawal use existing Human
Common API operations where the approved GUI has no mutation control; subsequent
GUI projection and actual MCP reads verify their outcomes.

### Corrected H2 result matrix

N2 independently reran the required production-composition, Human/API/MCP,
ordered/no-op, authorization/recovery, worker-failure, PDF, health/drain/outage and
same-state restart paths: all22 stages, journey11 and persistence1 passed. H2's
separate [scheduler job110775471605](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36987407999/job/110775471605)
passed its own dataset; it is not a browser/MCP dataset observation.

| Corrected requirement | N2 normal run | V2 capture run |
|---|---|---|
| Required actual composition/consistency/failure/restart matrix | PASS | PASS |
| Actual Chromium Japanese heading/body font selection | PASS, fixed CDP marker | PASS, same fixed marker on its own passed test |
| Long IANA label + both DST-fold instants at1280/1440 | PASS, six display-layout cases | PASS, six display-layout cases |
| Checkpoint aria-busy/transition/bounds assertions | PASS, scripted readiness | PASS for those assertions;01 selected-detail query is outside the generic aria-busy guard |
| File export validation and actual13-pixel review | No capture in N2 | Validation PASS; all13 inspected; scoped visual findings and limitations below |
| Final independent report review and report-R2 exact gates | Separate requirement | Independent report review GO; report-R2 gates NOT RUN |
| Explicit owner confirmation | NOT GIVEN | NOT GIVEN |

## 6. Actual pixel review — historical V1 FAIL; corrected V2 findings

Method: inspect the actual V1 PNG pixels, alongside same-run DOM/keyboard assertions.
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

Historical read-only diagnosis at H1, before the separately approved H2 remedies,
distinguished evidence from hypotheses:

- **Font/render prerequisite, probable:** the font stack names Hiragino/Yu Gothic/Noto Sans JP then system sans; there is no bundled webfont. The runtime workflow installs Chromium but no explicit CJK font. Japanese DOM assertions passed while glyphs render as boxes, consistent with missing font coverage rather than demonstrated UTF-8 corruption. Hosted font inventory and Chromium's selected font have not yet been collected. No font has been adopted or installed by this report.
- **Framing, confirmed:** the capture helper takes `fullPage:false` at the current scroll position without a framing reset. Views07 and09–13 omit important context. A corrected capture needs to show both target and result without hiding the actual state or altering product content.
- **Timestamp formatting, confirmed source inconsistency:** Home uses `Intl.DateTimeFormat('ja-JP', {dateStyle:'medium', timeStyle:'short'})`; Detail explicitly adds `timeZone:'Asia/Tokyo'`. Playwright sets Japanese locale without a timezone override. The nine-hour image discrepancy is consistent with this difference. Choosing a browser timezone alone would not resolve the product formatter inconsistency; the approved intended display contract must govern any fix.
- **Capture timing, unqualified:** view13 shows a success strip while a button label ends in an ellipsis. Because the Japanese label is unreadable, this is not classified as a product defect. A corrected capture must verify the intended final pending state has settled without changing focus or business state for appearance.
- **Context width, observed but cause unqualified:** view06 squeezes the right pane and wraps the filename awkwardly. Recheck at1440/1280 with verified glyph coverage before assigning the cause to product layout versus font metrics.

Source locators: [font tokens](https://github.com/AIrisu-072/knowledge-platform/blob/31b75d81941027f3c00be0617fb26cd0f6a9e18c/apps/document-web/src/design-system/tokens.css),
[Home formatter](https://github.com/AIrisu-072/knowledge-platform/blob/31b75d81941027f3c00be0617fb26cd0f6a9e18c/apps/document-web/src/routes/DocumentHomePage.tsx),
[Detail formatter](https://github.com/AIrisu-072/knowledge-platform/blob/31b75d81941027f3c00be0617fb26cd0f6a9e18c/apps/document-web/src/routes/DocumentDetailPage.tsx),
[capture helper](https://github.com/AIrisu-072/knowledge-platform/blob/31b75d81941027f3c00be0617fb26cd0f6a9e18c/tools/document-poc-runtime/visual-evidence.mjs) and
[Playwright configuration](https://github.com/AIrisu-072/knowledge-platform/blob/31b75d81941027f3c00be0617fb26cd0f6a9e18c/apps/document-web/playwright.runtime.config.ts).
A missing or inadequate image is not a PASS inferred from another run.
No benchmark was performed; workflow/test timings are operational observations,
not interactive latency measurements or an approved performance threshold.

### Reviewed remediation and actual N2 proof

H2 adds the verified Apache-2.0 Kosugi4.002 payload only to the disposable runner's
private Fontconfig directory. Product font-family/CSS is unchanged by that font
repair; only FONTCONFIG_FILE is exported, preserving toolchain/XDG locations.
The immutable font/license/attribution pins, source review and coverage/weight
limits are in the [font qualification record](../../research/document-japanese-runner-font.md).
N2's passed CDP marker proves actual selection for the tested Japanese heading
and body nodes. It does not certify all text, symbols or visual hierarchy.

The same13 checkpoints now wait for aria-busy work and finite transitions, preserve
focus, and assert bounds.01–05 remain900px fixed-height captures (02 at1280px width);
06–13 are explicitly full-page1440px captures with height900–4096px and8MiB maximum.
No scroll/focus/business state is changed to improve a picture. Home preserves
browser-local conversion; Detail preserves Tokyo conversion. Visible zone and
instant-specific UTC-offset labels remove ambiguity without choosing a new global
timezone policy or changing inputs/API instants. N2's six browser layout cases
cover the long real IANA name and both repeated DST-fold instants at both widths.
These scripted proofs remain separate from V2's actual pixels.

### Corrected V2 pixel findings

Three independent assistant inspections read the13 originals. This is direct
pixel review alongside actual scripted assertions, not an external participant
study, WCAG certification or performance benchmark. PASS below is scoped to the
named concern and retains each limitation; it is not owner acceptance.

| File | Actual V2 observation |
|---|---|
| `01-list-context-1440.png` | PASS for readable list/loading context; fully ready selected-detail state NOT PROVEN. Selected row, file, revision and UTC timestamp are readable while 「文書情報を読み込み中…」 remains visible. |
| `02-list-focus-return-1280.png` | PASS for1280 ready selected context and return focus. Version3/Revision3.0, capabilities and UTC label are readable. The long English synthetic list title is clipped within its cell; selected Japanese context remains readable. |
| `03-detail-overview-1440.png` | PASS. Header, currentVersion2/Revision2.0, original file and metadata are readable. Asia/Tokyo, UTC+09:00 visibly explains the nine-hour difference from the list. |
| `04-revision-version-1440.png` | PASS. Content Version and formal Revision explanations, both revision entries and comparison controls are readable; selected-tab focus is visible. Zone/offset wraps without overlap. |
| `05-comparison-1440.png` | PASS. Document/version identity,1.0→2.0, full comparison coverage and both Japanese changed passages are readable. |
| `06-version-file-selected-1440.png` | PASS. Full target/header, Version2 base, selected primary171B and create control are visible. The squeezed context-pane transition seen in V1 is absent. |
| `07-publication-ready-1440.png` | PASS. TargetVersion3, immediate-publication choice, confirmation checkbox, file and publish action are visible together. |
| `08-publication-confirm-focus-1440.png` | PASS for readable confirmation/target/focus; full-page capture limitation disclosed. The fixed modal backdrop begins around y556, leaving above-current-viewport content unshaded; the dialog and focused confirm action remain clear. |
| `09-publication-success-1440.png` | PASS for authoritative success/currentVersion3 and returned focus, with a completion-state UX limit. The authoring target becomes Version未選択/原本ファイルなし after the only WORKING version is published; this does not mean its committed file was removed. |
| `10-access-policy-effective-draft-1440.png` | PASS. Header, inherited effective policy, separate editable draft, read-only Agent grants, reason and save/discard controls are readable. The reason field is focused. |
| `11-occ-conflict-1440.png` | PASS for actionable conflict/retained input. stale.txt51B, conflict guidance, reload and same-content retry are visible. Full trace ID is tightly adjacent to the reload button; copyability was not tested from pixels. |
| `12-permission-denied-file-retained-1440.png` | PASS for hidden-denial/retained input. race-denied.txt35B, non-disclosing error and both recovery controls are visible. Same cramped trace/button spacing; no proven lost characters. |
| `13-permission-restored-retry-success-1440.png` | PASS. TargetVersion2, cleared selection, explicit creation success and normal create-button label are visible. The V1 upload-pending ellipsis is absent. |

Source-grounded limits:

- **01 capture readiness:** [DocumentHomePage.tsx:176](https://github.com/AIrisu-072/knowledge-platform/blob/6103e4d4e3bb0d45ba03e1d2935492de7f11394a/apps/document-web/src/routes/DocumentHomePage.tsx#L176) displays selectedDetailQuery.isPending separately from safe list-row metadata. LoadingState is a role=status paragraph without aria-busy, so the generic no-aria-busy/finite-transition guard does not prove that query completed. GUI design §§24–25 permit honest Loading/Ready states and stable selection.01 is loading-state context evidence;02 is a separate same-run ready-context observation. Do not call01 fully settled or replace its pixels with02.
- **08 full-page representation:** the scrim is position:fixed/inset:0 ([DocumentDetail.module.css:978–985](https://github.com/AIrisu-072/knowledge-platform/blob/6103e4d4e3bb0d45ba03e1d2935492de7f11394a/apps/document-web/src/routes/DocumentDetail.module.css#L978)). The1440×1468 image includes content outside the actual900px viewport. Its viewport-local backdrop is not evidence that the modal left the live viewport partly uncovered. No altered scroll, CSS or image composition was used.
- **09 completion presentation:** the existing authoring Version query includes only WORKING rows ([document_history.rs:490](https://github.com/AIrisu-072/knowledge-platform/blob/6103e4d4e3bb0d45ba03e1d2935492de7f11394a/crates/document-repository-postgres/src/document_history.rs#L490)). After publication it has no WORKING selection; `chooseVersion` returns undefined, and the publication template renders its existing target/file fallbacks ([DocumentDetailPage.tsx:544–547,1042–1053](https://github.com/AIrisu-072/knowledge-platform/blob/6103e4d4e3bb0d45ba03e1d2935492de7f11394a/apps/document-web/src/routes/DocumentDetailPage.tsx#L544)). The actual committed Version3/file and success are independently proven. This is a visibly awkward completion state, not demonstrated false success, data deletion or an approved new workflow design.
- **11/12 trace spacing:** ApiFeedback renders the full trace immediately before an inline reload button ([ApiFeedback.tsx:22–23](https://github.com/AIrisu-072/knowledge-platform/blob/6103e4d4e3bb0d45ba03e1d2935492de7f11394a/apps/document-web/src/components/shared/ApiFeedback.tsx#L22)), without a gap. The originals show32 hex characters and no source truncation rule. Error guidance, retained input and recovery controls remain readable; generous spacing or trace copyability is not claimed.

The three inspections reached scoped visual GO with these residuals disclosed
for owner review. No reviewed mandatory clause was
shown to fail because safe row data remains visible during an honestly labeled
query, or because of the bounded full-page/backdrop, completion-state and spacing
observations. The weaker generic selected-query readiness coverage is explicitly
retained as an evidence limitation; no criterion or runtime assertion was relaxed
for this report. There is no claim that every rendered state is ideal or every
possible layout/accessibility case has been tested.

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
problem shapes prevent existence/content disclosure. V1 completed all nine Agent groups with actual-stdio provenance and no reported
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
server/browser acceptance. H0 and V1 independently verified the supported single-line fixture in their
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

Read-only GitHub/source observations refreshed2026-10-02 09:26:11–09:27:23 UTC.
No material state change was observed. All three PRs remain OPEN, Draft and
unmerged. These are Search's own heads; they do not qualify a combined Document/Search runtime.

| State | Branch / exact head | Current hosted result |
|---|---|---|
| Qualified historical Phase D, Draft [PR33](https://github.com/AIrisu-072/knowledge-platform/pull/33) | `feat/search-discovery-platform-v0-d` / `4892ba5d2736b35bf95de25f834f016609d2e0d4` | CI36665497017, PoC36665497015, Sandbox36665497041 SUCCESS |
| Completion program, Draft [PR34](https://github.com/AIrisu-072/knowledge-platform/pull/34) | `feat/search-platform-completion-program` / `80a47960d025e4dfdea1eacade28b15d218725ff` | [CI36680522098](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36680522098) SUCCESS; program acceptance incomplete |
| Current held WIP, Draft [PR40](https://github.com/AIrisu-072/knowledge-platform/pull/40) | `feat/search-platform-cloud-continuation-20261001` / `a945fbd32145a3109e35cb9cb056cea052698138` | [CI36945866252](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36945866252) and [PoC36945866208](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36945866208) FAILURE; [Sandbox36945866263](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36945866263) SUCCESS |

Authoritative current-head paths:

- [Design](https://github.com/AIrisu-072/knowledge-platform/blob/a945fbd32145a3109e35cb9cb056cea052698138/docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design.md) and [approval](https://github.com/AIrisu-072/knowledge-platform/blob/a945fbd32145a3109e35cb9cb056cea052698138/docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design-approval.md)
- [Production plan](https://github.com/AIrisu-072/knowledge-platform/blob/a945fbd32145a3109e35cb9cb056cea052698138/docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-production-implementation.md) and [approval](https://github.com/AIrisu-072/knowledge-platform/blob/a945fbd32145a3109e35cb9cb056cea052698138/docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-production-implementation-approval.md)
- [Phase status](https://github.com/AIrisu-072/knowledge-platform/blob/a945fbd32145a3109e35cb9cb056cea052698138/docs/superpowers/execution/search-discovery-platform-v0-status.md), [program status](https://github.com/AIrisu-072/knowledge-platform/blob/a945fbd32145a3109e35cb9cb056cea052698138/docs/superpowers/execution/search-platform-completion-program-status.md), and [current held checkpoint](https://github.com/AIrisu-072/knowledge-platform/blob/a945fbd32145a3109e35cb9cb056cea052698138/docs/superpowers/programs/search-platform-completion/draft-publication-checkpoint-20261001.md)

Known continuation failures include missing `outbox_delivery::observe`, strict
Clippy large-enum diagnostics, eight wildcard path dependencies and yanked
`yoke-derive0.8.3`. Scanner self-test/history stages now pass their own checks;
that is not overall security PASS. PR33 success does not qualify PR40 WIP.
PR34's successful program-contract CI also does not qualify PR40. The held
checkpoint remains authoritative, although its earlier pending-CI descriptions
are historical; the exact terminal failures above were re-read. These are
separately held Search work, not permission for E3 to repair it.

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

The reviewed H2 tree combines the isolated framing, font and timestamp display
slices with the narrow browser geometry proof. Fresh local runtime-helper129/129,
GUI15 suites/57, application/runtime types, schema freshness, actionlint, MCP
bundle build and full range checks passed. Playwright collection found the prior10
journeys plus the dedicated11th geometry test. Independent final source review was
GO; no actual local Chromium or Rust run was claimed. N2 subsequently supplies
actual hosted font/layout/runtime proof. Three existing webpack performance
advisories remain, without an invented interactive performance SLO.

Independent factual/privacy review of the five-document scope, separate receipts
and all13 original pixel findings is **GO**, with no unresolved Important finding.

Remaining: publish report R2 and verify its own exact applicable hosted gates, then
present READY FOR OWNER REVIEW if no material finding remains. Owner confirmation
has not been given. Artifact creation and expiry were both read from actual
GitHub metadata, not derived from each other. Pixel findings do not derive from
the source tests or N2 marker; H1/V1's historical failure remains intact.

Independent Audit limitation: the approved
[Management reason-retrieval clause](https://github.com/AIrisu-072/knowledge-platform/blob/d71753d46590bb4406a1c0b74894ab90a27a6c88/docs/superpowers/specs/2026-09-28-document-management-basics-v0-design.md#L136)
requires the T5–T8 ledger reason to be retrievable. The separate
[PR45 producer-compatibility investigation](https://github.com/AIrisu-072/knowledge-platform/pull/45)
remains qualification correction only at its observed remote head `f616b720…`.
As confirmed2026-10-02 09:31 UTC, no new producer/ledger change qualifies the
retrievable-reason requirement: the last-known legacy durable ledger retains
digest/result, and normal ACL reason handling remains deferred. This is scoped
independent Audit/legacy-producer work, not an added PR43 acceptance gate.
Document history/persistence assertions in V1, N2 or V2 do not establish that
separate contract. No Audit fix or full Audit qualification is claimed here.

C0's separately stated G9 closure gap remains an original-request deliverable;
known Search WIP remains separately held. Neither is silently promoted by H2.
The report does not declare the entire original request complete.

After actual evidence, independent review and report-R2 gates pass, use
**READY FOR OWNER REVIEW: H2/V2 qualified, report recorded at report R2, final
acceptance awaiting owner confirmation.** Only explicit owner confirmation can
advance the decision to accepted for that named scope. Record both exact commits,
runs and the actual decision in the PR receipt without changing frozen H2.
All PRs remain unmerged and undeployed. No statement authorizes Organization
Client, production identity, Search implementation, Agent writes, merge or deploy.

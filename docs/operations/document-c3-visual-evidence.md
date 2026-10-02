# C3 synthetic visual evidence candidate

Status: **BOUNDED ADOPTION APPROVED / CAPTURE PROCEDURE**. Actual per-source
capture, artifact, pixel-review and cleanup outcomes are recorded in the
[acceptance report](../superpowers/execution/document-poc-acceptance-v0-report.md).
This extends E0 Task E3. A generated file, parser unit test, browser discovery or
old mocked snapshot is not actual runtime or human visual-review evidence.

## Capture boundary

Set `KP_POC_CAPTURE_VISUAL=true` only for the ordinary owned composition-root
harness, without `--prebuilt`. The runner still builds the production binaries and
GUI, creates/seeds a fresh harness-owned disposable loopback database, and runs its
existing actual Human/Agent journeys and lifecycle checks. No backend route is
mocked. Visual mode rejects any `TEST_DATABASE_URL` input before evidence setup,
commands or database use, even with `KP_POC_DISPOSABLE_DATABASE=true`. An external
disposable acknowledgement cannot prove the folder tree contains only synthetic
content. Ordinary nonvisual external-database acceptance remains unchanged.
No Rust build or actual runtime run was performed to prepare this candidate.

The run-local context selects a newly created private `visual-checkpoints`
directory with the explicit `harness-owned-disposable-loopback` proof literal.
The exporter independently requires `report.database.ownership === "harness-owned"`. Actual `page.screenshot` calls occur immediately after the named
assertions. They are not failure-handler or reporter attachments. Visual mode
turns off automatic screenshot/trace/video diagnostics. Existing JSON/XML reports,
logs, context/seed manifests, executables, originals and storage stay outside the
export selection. Nonvisual runtime behavior is retained.

Desktop Chrome's device preset previously overrode the top-level viewport. The
project now explicitly sets 1440×900 and device scale factor 1 after that preset;
journey cases explicitly set their starting viewport as well. The focus-return
checkpoint explicitly uses 1280×900. Capture verifies the actual page origin and
viewport before requesting checkpoint-specific CSS-scale PNG bytes.

Before each checkpoint, including ordinary non-capture runtime runs, the helper
waits up to the existing15-second UI assertion budget for no `aria-busy=true` and
no running/pending finite Web Animations transitions. It observes actual page
geometry, rejects horizontal overflow and bounds the full-page height. It does
not change scroll, focus, product CSS, animation timing or business state. During
capture, the settled active element must remain identical and UI must still be
settled after screenshot collection. The byte parser independently checks the
resulting PNG dimensions. A timeout, oversized page, renewed pending state or
focus change fails the checkpoint before any export. These are harness checks;
actual hosted layout and pixels remain qualification evidence.

The first H `31b75d81941027f3c00be0617fb26cd0f6a9e18c` capture exported valid files
but failed Japanese readability. Images07/09–13 lost context through viewport-only
framing;06 showed a partially collapsed context pane consistent with its transition, and13 could
precede upload invalidation settlement. This correction addresses capture
settlement/framing only; it does not claim to repair fonts or product timezone
ambiguity, or retroactively qualify the failed artifact. The same13 filenames,
synthetic owned-run guard, fixed uploader/retention and deliberate activation gate
remain in force. No ordinary run uploads images and no old event is retriggered.

Exactly these checkpoints are eligible. Viewports remain1440×900 (02 uses1280×900). Images01–05 are fixed-height viewport captures;06–13 explicitly capture the full page, with height900–4096px, to retain document/header and action/error context together:

| Filename | Passed assertion immediately before capture |
|---|---|
| `01-list-context-1440.png` | Selected synthetic document context panel |
| `02-list-focus-return-1280.png` | Keyboard detail→list return restores selected-row focus |
| `03-detail-overview-1440.png` | Document detail heading |
| `04-revision-version-1440.png` | Revision tab focus, formal revision heading and two persisted revisions |
| `05-comparison-1440.png` | Comparison heading and full Human/Agent comparison digest equality |
| `06-version-file-selected-1440.png` | Synthetic upload input retains the selected filename |
| `07-publication-ready-1440.png` | Required confirmation checked and publish enabled |
| `08-publication-confirm-focus-1440.png` | Confirmation dialog's final action has keyboard focus |
| `09-publication-success-1440.png` | Authoritative publication success and focus returned |
| `10-access-policy-effective-draft-1440.png` | Effective grants shown, explicit draft mode and reason set |
| `11-occ-conflict-1440.png` | Actual 409, conflict feedback, no success and no duplicate version |
| `12-permission-denied-file-retained-1440.png` | Actual 404 DOCUMENT_NOT_FOUND, retained file, unchanged state and no duplicate |
| `13-permission-restored-retry-success-1440.png` | Restored policy, actual 201, visible success and one added version |

The screenshot labels identify asserted checkpoints, not independent human
judgments of legibility, clipping, focus-ring visibility or design fidelity.
Inspect actual pixels later and record those findings separately.

## Validation and local export

Only after all runtime stages and cleanup succeed, with a clean source head and
built-in-this-run provenance (HEAD and porcelain are rechecked immediately before
export and must match the clean run start), the runner validates the entire fixed 13 set. It
rejects missing/extra basenames; symlinks in the directory ancestry or files;
hardlinks; nonregular, executable, nonprivate or nonowned files; files over 8 MiB;
wrong width or height (01–05 exactly900px;06–13 between900 and4096px); wrong signatures/chunks/CRCs; text/EXIF/other
ancillary metadata; malformed/truncated or trailing data; and oversized inflation.
Supported pixels are Chromium's 8-bit noninterlaced RGB/RGBA PNG form.

Library selection: use Node's existing `zlib.crc32` and bounded `inflateSync` plus a
small structural PNG validator. No new dependency is added. Playwright's private
bundled image decoder is not a supported public API, and merely reading IHDR
would allow malformed or metadata-bearing data to masquerade as a screenshot.
Parser fixtures are generated inside unit tests and explicitly cannot count as
browser evidence.

Validated bytes, rather than mutable source paths, are written to a fresh 0700
staging directory with 0600 files, then renamed to the private fixed
`tools/document-poc-runtime/.state/visual-export` directory (or the same fixed
basename under the explicitly selected evidence parent). Only 13 PNGs enter it.
The exporter refuses an existing destination; it never deletes or reuses older
evidence. An export failure fails the capture-enabled run. A failed/incomplete
runtime never exports its partial checkpoint set. Review remains `NOT RUN` even
when validation succeeds. The full working run directory must never be uploaded.

## One deliberate current-review activation

The approved destination is the public repository's Actions artifact store;
signed-in repository readers can download the fixed synthetic pixels. Approval
covers this PR43 review only and one-day retention. The official action is pinned
to `043fb46d1a93c77aae656e7c1c64a875d1fc6a0a` (v7.0.1); its pin-specific license and
residual-risk decision is recorded separately. It does not authorize arbitrary
future actions, new package versions, screenshots from other data, or standing
uploads on ordinary CI runs.

The existing CI workflow preserves `opened`, `synchronize`, `reopened` and push
behavior and adds the supported `pull_request: labeled` activity. The gate is
false by default. It becomes true only for an open PR43 in
`AIrisu-072/knowledge-platform`, with the same non-fork head repository and branch
`feat/document-poc-acceptance-v0-e1`, a newly added label exactly
`c3-visual-` followed by that event's 40 lowercase hexadecimal head SHA, and
`run_attempt` equal to 1. The gate window is
`2026-10-02T04:31:00Z <= now < 2026-10-03T04:31:00Z`.
It verifies the checked-out local HEAD equals the event head and the working tree
is clean. CI checks out the event head explicitly, rather than the PR merge ref.

Both capture and upload consume this same immutable step output. A label merely
remaining on the PR does not enable synchronize, reopen, push or rerun capture.
Malformed/expired input disables capture without printing the event body. The
gate performs no network request and reads no token. No job permission or secret
is added; `contents: read` and non-persistent checkout credentials remain intact.

After reviewed source publication, the operator must verify the exact current
head, its required policy/security gate results, and the absence of an already accepted capture, then apply that head-specific
50-character label once. The GitHub connector has additive string-label support;
label creation/acceptance must be confirmed from its actual result. Do not remove
and re-add automatically. A stateless event gate cannot stop a second deliberate
label removal/re-addition inside the window, so the operator must not do that.
An uncertain label result requires a read of current labels/runs, not a blind
retry. Do not extend the window or broaden the trigger silently.

Only overall job success and successful owned-run validation/export permit the
pinned uploader to run. Its input contains the 13 literal PNG paths above, no
directory/glob; `retention-days: 1`, `if-no-files-found: error`, `overwrite: false`
and compression level 0. `include-hidden-files: true` is needed only for `.state`;
the owned exporter excludes hidden files, symlinks, extra files and metadata.
No JSON/XML reports, logs, traces, manifests, credentials or failure screenshots
are uploaded. The old every-run review-only patch is not used.

## Receipt and actual visual review

After upload, retrieve the supported artifact metadata and record exact source
head/tree, action pin, workflow run/attempt, artifact ID/digest, 13-file validation,
and actual `created_at`/`expires_at`. Confirm the returned expiry honors the
approved one-day limit before claiming retention verification. Administration
allowlist/default settings remain unobservable through the supported connector;
no alternate route or setting change is allowed. An action-policy rejection is a
closed failure, not permission to bypass it.

Download only this artifact for temporary local review. Inspect all actual
pixels for legibility, clipping/overflow, focus-ring visibility, selected state,
dialog placement, error/retry feedback, and consistency at 1280/1440 widths.
Record PASS/FAIL per checkpoint and concrete findings independently of automated
assertions. File existence and PNG validation do not complete visual review.
Do not commit generated PNGs, mirror them into Library, or retain local review
copies beyond the approved period. Real composition still uses the same exact
head/database/storage/run for Browser/API/stdio evidence; no mocked substitute.

The source/security/workflow reviews and actual runtime, artifact and pixel
results belong to their separately named exact heads/runs in the acceptance
report. Preparation-time source review is not an actual capture or pixel result.
For each artifact, record its actual creation/expiry and temporary local-copy
cleanup separately. Delete local review copies no later than that artifact's
approved expiry and record verified completion; do not claim cleanup in advance.

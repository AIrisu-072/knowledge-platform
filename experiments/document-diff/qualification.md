# Document Diff v0 qualification ledger

## DIF-04 two-source worker shell / runner candidate

The frozen design allows the DSI sandbox seal to be reused. The Diff runner passes two distinct read-only source descriptors (FD 3 and FD 4), a bounded request on stdin, and a private scratch directory. The trusted runner checks both raw hash/size bindings before launch; the worker checks them again before any adapter. It clears inherited environment and marks unlisted descriptors close-on-exec. Worker startup requires the DSI Linux Landlock/seccomp seal; production startup is rejected on non-Linux hosts.

Candidate profile `diff-resource-v0`: each source 256 MiB, combined sources 512 MiB, request 64 KiB, result 16 MiB, stderr 1 MiB, wall 30 s, CPU 25 s, address space 4 GiB, monitored temporary tree 2 GiB. These are **candidate bounds**, not production measurements. Node/depth/candidate/change limits are qualified in DIF-05/DIF-15.

Local macOS evidence: portable worker shell tests cover independent raw bindings, malformed/oversized request, unsupported-format unverified result, and panic containment. Strict Clippy and formatting pass. The runner rejects production construction on macOS. Linux tests for fresh process, network/exec denial, environment/descriptor isolation, timeout and output bounds are written but require the hosted Sandbox gate. No Diff format adapter or parser dependency is promoted at DIF-04.

Outstanding: hosted Linux compile/runtime, Landlock/seccomp enforcement canary, exact limits and 1-over checks, native-runtime warmup for later PDF adapter, representative large-document measurements. Until these pass, the candidate profile is not qualified for production disclosure.

## Delivery Unit A hosted isolation evidence

At exact head `ddc3c1b6e94557ed13831ed343f5d86d5bc97cec`, standard CI `36507722838`, DSI Sandbox Preflight `36507722786`, and DSI PoC regression `36507722787` succeeded. The Linux standard CI rust-test job ran all three `document-diff-runner::runner_isolation` canaries, including fresh sandbox/network/exec denial and timeout/output failure handling. The earlier A head failed on a Linux-only test binding shadow and the pre-existing mise OSV Scanner signer setup; both were fixed before this GREEN head. This qualifies the Linux isolation behavior exercised by those canaries. Numeric Diff resource bounds and representative large inputs remain candidates until DIF-15.

## DIF-07–09 format candidate qualification

- TXT: pinned `encoding_rs 0.8.41` and `unicode-normalization 0.1.25`; CRLF/LF and NFC relations, changed/add/remove raw byte spans, invalid UTF-8 fail-closed. Focused `text_diff` 5/5.
- CSV: pinned `csv 1.4.0` and existing Unicode normalization; quote noise, cell/row/column locators, unique reorder, duplicate ambiguity, inconsistent rows, ambiguous delimiter, and row-limit reason. Focused `csv_diff` 6/6. The v0 comparator accepts a delimiter only when one of comma, semicolon, tab, or pipe is the unique consistent multi-column parse for each source. Any ambiguous or unsupported delimiter profile remains unverified; no delimiter is guessed when more than one interpretation works.
- HTML: pinned `html5ever 0.39.0` and `markup5ever_rcdom 0.39.0`; visible text/link/image/heading/table changes, whitespace/decorative noise, script non-execution, script-only fail-closed, bounded DOM traversal. Focused `html_diff` 4/4. DOM paths identify original parsed nodes; where no safe node is available, the whole ContentItem is unverified.

All three format libraries were already selected for DSI production, but these Diff-specific fixtures and bounds are the promotion evidence for Document Diff. B local `verify:fast` passed 599/599 Rust tests (five default skips) with the pinned PDFium library path. At exact head `7dde25a56784f3dd79b0a994d869ce06bb32acad`, standard CI `36509161078`, Sandbox `36509161061`, and DSI PoC `36509161323` all succeeded. DIF-15 resource/large-document measurements remain pending.

## DIF-10 DOCX local qualification

The DOCX comparator uses the existing qualified DSI `DocxAdapter` as a semantic guard and pins `office_oxide 0.1.11`, `quick-xml 0.42.0`, and `zip 8.6.0`. It emits original OOXML paragraph/table-cell paths, parent-part paths for header/footer/notes/image/link/section, and `ContentItem` unverified regions when alignment or OOXML semantics are uncertain. Unique stable paragraph IDs support move-plus-edit; duplicate or unsupported correspondence is not guessed. Editorial provenance changes are ancillary, separate from the content verdict. Test-only RED `014a4cda8c29d5ae45ab98e1bb7829ae57f39bfd`; local GREEN `19343d089747fe7f038ea7009d8348d5c94c1452`: `docx_diff` 6/6, core protocol 6/6, Application contract 9/9, focused strict Clippy pass. Hosted C gate and final resource measurements are pending.

## DIF-11–13 Office format local qualification

- XLSX: test-only RED `694bff0b450538934807f4ad63e31a77f8b9ba9b`, local GREEN `934dfc0a038849fb5bf0a55187e69ad1c3ef5b9a`. `xlsx_diff` 7/7 and focused strict Clippy PASS. Reuses qualified DSI spreadsheet guard and pinned `rxls 0.1.3`; distinguishes value/formula, sheet order/visibility, named range, merge/table/chart/image/link/external source. Cached value and decoration noise are equal. Unique row reorder is identified; duplicate row movement is unverified.
- XLSM: test-only RED `5a9fb0f156743d570cb182b86ba3e3a8d32161a7`, local GREEN `d062668046bc14ec7d336a9883eee663202709ad`. `xlsm_diff` 4/4, VBA reference/module unit 1/1, existing DSI VBA regression 6/6 and focused strict Clippy PASS. The format-specific DSI VBA projection uses pinned `ovba 0.7.1` and `tree-sitter 0.25.10`; macro source is never executed. Worksheet and VBA changes are separate; invalid source is unverified.
- PPTX: test-only RED `1843f2547aae7442f86d09198dddc9f2ddd3cacc`, local GREEN `54a935880321bbefdd9d4b195df0ea8a91b435f7`. `pptx_diff` 5/5, duplicate-shape ambiguity unit 1/1, existing DSI PPTX regression 8/8 and focused strict Clippy PASS. The format-specific DSI projection uses pinned `office_oxide 0.1.11`, `quick-xml 0.42.0`, and `zip 8.6.0`; slide-parent locators are used when object identity is not certain. C common/hosted gates and final resource measurements are pending.

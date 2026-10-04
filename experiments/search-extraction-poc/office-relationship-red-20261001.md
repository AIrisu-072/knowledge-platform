# P1-Q01 next Office relationship corpus: RED receipt

This is **new work after** the closed 45-case Linux bounded PoC receipt. It does not alter that manifest, its raw fixtures, its qualification JSONL, or the previously reviewed binary/source hashes.

- Current bounded binary SHA-256: `9af6b5e7dc79f0bb66f1d9c8a385711945387c0045101202a55c8956b8d8801e`.
- Exact new tests: `tests_next/test_office_relationships.py`, with independent Python stdlib OPC relation/content-type/part validation in `tests_next/office_oracle.py`; run explicitly with `P1_QUALIFIER_BIN` and the pinned `PDFIUM_DYNAMIC_LIB_PATH` environment from `linux-qualification-20261001.md`.
- RED log: `/tmp/p1-office-relationships-red-20261001.log`. Ten tests ran: six expected-control cases pass, four required cases fail. A related PPTX notes traversal target is rejected in the control set.
- Newly built raw bytes are synthetic, in memory, with recomputed request SHA-256. The independent oracle validates relationship ID/type/target, referenced part existence/content-type/XML, and the old fixed raw-text/locator oracle validates positive Units. The tests are **not** counted in the prior 45-case green suite.
- These mutation probes place bytes in a temporary fixture root; `parser_build_sha256` in their row output reads that root at runtime and therefore is not build provenance. The executed binary's separate SHA-256 above identifies this RED run.

Blocking observations:

1. A workbook shared-string relationship pointing at absent `xl/missing-shared-strings.xml`, or declaring a wrong relationship type, still yields `Supported`, 4 matched cells, `qualified=true`, because the parser reads the fixed `xl/sharedStrings.xml` package member while ignoring the declared type/target. A correctly typed/targeted relation retains the four expected cells.
2. A DOCX main document with a section header reference, typed `word/_rels/document.xml.rels`, declared `word/header1.xml`, and actual header bytes yields `Unsupported` / Unit zero instead of locatable `Partial` main-body Units.
3. A PPTX slide with typed notesSlide relationship, declared `ppt/notesSlides/notesSlide1.xml`, and actual notes bytes yields `Unsupported` / Unit zero instead of locatable `Partial` slide Units. Missing or wrong-MIME note controls already fail closed.

The old standalone header/notes fixture parts lack full relationship topology, and the old rich shared-strings fixture lacks a workbook shared-string relationship. Thus those 45 cases cannot justify general Office OPC admission. Do not edit their closed receipt to mask this. Next isolated PoC step: validate any supplied relationship IDs/types/targets/content types and part bytes before `Supported` or located `Partial`; reject broken targets atomically. Keep unknown topology fail closed, add missing/wrong target negative controls, independent oracle and a separate v2 qualification receipt. No production Cargo or `search-extraction-worker` dependency promotion from this RED.

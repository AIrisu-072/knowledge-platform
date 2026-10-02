# Synthetic publication PDFs

These two native-text PDFs qualify the successful publication journey. They are
outputs of the `minimal_text_pdf` / `write_pdf` recipe in
`experiments/document-semantic-inspection/tests/support/pdf_fixture.rs`, with
producer `Synthetic runtime acceptance`, and text `Page A` / `Page X`.
Both are 684 bytes; their only byte difference is A → X at offset311.

- base.pdf SHA-256: b766f4e522d96298952d4c0c63bc97a99ef9323e422f1c2eb5364e1ab2100a15
- text-change.pdf SHA-256: 76bb9d774387019d5c8432604dddd986a33020fd1aa399601bf317458a3b007e

The PDF worker tests check PDF detection, empty comments/tracked changes/signatures,
no external dependencies, raw-byte binding and distinct semantic fingerprints.
The Diff test checks full coverage and a native-text change. The runtime keeps
actual publication, displayed Diff and unchanged-original download assertions.
The original richer DSI fixture remains in a separate negative runtime case that
requires the existing embedded-comment publication quality rejection.

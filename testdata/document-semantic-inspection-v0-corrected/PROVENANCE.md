# Corrected Document Semantic Inspection v0 qualification corpus

- Source head: `e309cbeb9f9075c5c3b900df8905f8ae62490337`
- Source tree: `experiments/document-semantic-inspection/fixtures` at that exact commit
- Exact-head DSI PoC qualification: `36161063199` — SUCCESS, including all 91 manifest cases
- Manifest SHA-256: `d1d4514e24218db4fab24aea7302a187689e06d5ee1e26cd9ef40516aae51b9d`

The original `a4fcef1...` corpus is preserved at `testdata/document-semantic-inspection-v0/`. Its 20 DOCX fixtures contained a PNG with an invalid checksum. The Task 5 PoC repair corrected only those synthetic PNG bytes and their manifest hashes; it kept all 91 relation/error expectations. Production's checksum-validating PNG decoder must reject the original malformed bytes. This corrected and requalified copy is the executable 91-case parity input.

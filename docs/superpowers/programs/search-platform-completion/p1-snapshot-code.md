# P1-S01 authoritative snapshot — parent implementation receipt

- Uncommitted feat/search-platform-completion-core@80a4796, worker completion_p1_body_snapshot.
- Reported actual RED: missing authoritative_items API (six E0609); type-only stub then real PostgreSQL no-authoritative-Part refusal test failed as expected.
- Reported GREEN: body_snapshot6/6, postgres_snapshot4/4, translation_contract8/8, generation_races3/3, access_visibility1/1, relation_projection11/11. outbox_indexing19/20 and vertical_slice4/5 initially failed on legacy hand-built item0 fixtures; minimal authoritative FileObject/Representation/ContentItem additions then only each failed case rerun1/1 GREEN. Original business assertions preserved.
- Reported strict SourceDoc all-target Clippy, package fmt and diffcheck PASS. This parent receipt records the worker completion; independent actual cached PostgreSQL recheck is a separate p1-authoritative-snapshot-review.md gate.
- SHA256 crates/search-source-document/src/model.rs: `028e6d05de6b0f7bf78d63d7c0c6ac03a8a1efb3804f9c94e791a9b94a6069e3`
- SHA256 crates/search-source-document/src/postgres.rs: `79832c0e6debad719c251cbef48ae9bc5cb28f5316798552f45f59465107ad77`
- SHA256 crates/search-source-document/src/lib.rs: `db48030ab6ae34dee1f08fb4529f9d7052f5fcac026503dc715f3abb7ca6fbdc`
- SHA256 crates/search-source-document/tests/body_snapshot.rs: `a877a66a1fcf9d163d58193216d952defe43ab710f9f3722e9cd23f5558a21c0`
- SHA256 crates/search-source-document/tests/postgres_snapshot.rs: `e7ff7b10d05a9caaf067060d3e452945da35f3242b2e085d29b5d090888e91fb`
- SHA256 crates/search-source-document/tests/outbox_indexing.rs: `62c6816d8cfb4d4833bf1ef5d5cd0735861b1cce237a832f43039d6a280239d7`
- SHA256 crates/search-source-document/tests/relation_projection.rs: `fe42db4a66d3c17addb414180033f75d68b2ebfdd421ae047cadc65ea257e4e6`
- SHA256 crates/search-source-document/tests/vertical_slice.rs: `a59f70a8b526711b06d8ae3d090a72ebaa5a416ab4d6eaa3ba14b6b9c6b8e51f`
- S01 provides same-read-only-snapshot Version/Part/representation/raw metadata and checks absence/duplicates/path/ordinal/hash/size/MIME/FileId/StorageKey. Raw FileStorage bytes, extractor/budgets/profile/NativeLocator round trip/current body Read/Live bundle/evidence still require subsequent P1 tasks.

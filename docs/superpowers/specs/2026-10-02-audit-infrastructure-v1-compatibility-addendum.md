# Audit Infrastructure v1 — bounded compatibility addendum

Status: INDEPENDENT REVIEW PENDING; required before source admission implementation. Original frozen design `a1d2002afb7525f19f40138a82365bc975f493ed` and plan `3fd2d2ef2b8891dd63345233c5f67f69412dfbed` remain unchanged.

## Source-node budget

Actual `document_diff_access.rs` stages three `[u8;32]` digests from `document-application/src/document_diff/model.rs` and the result digest. These arrays already require96 numeric children, so the original64-total-node source admission ceiling would incorrectly quarantine required, bounded Diff evidence.

Refine only total source traversal nodes from64 to512. Keep depth≤4, object fields≤64, key bytes≤128, string/scalar bytes≤512, uncompressed storage≤24KiB, serialized source wire≤24KiB and envelope≤32KiB. Native numeric integer/signed64 preflight and compression rejection remain. All stages remain procedural and precede raw client transfer/hash. Bounded traversal stops at node513 without walking an unbounded tail.

The intermediate rendering bound remains256KiB. Uncompressed stored source bytes cap raw strings/keys; escaping contributes at most6×24KiB. At most512 signed64 numeric nodes contribute at most20 characters each, conservatively10KiB, plus bounded JSON punctuation/outer field names; the total is below256KiB. Qualification must verify the estimate against worst-case escaping, integers,512-node and513-node cases as well as compact exponent/high-scale adversarial values. No runtime limits are removed.

## Legacy correlation preservation

Existing `document_diff_access` accepts an optional correlation token and stages it in `trace_id`; its real-PG test uses `diff-test`. It is evidence but not W3C trace context. A qualified32-character nonzero lowercase hexadecimal value maps to `correlation.trace_id`. Other bounded identifier-shaped legacy tokens map unchanged to `correlation.legacy_correlation_id`; they must never become a newly generated/assumed valid trace. Unknown request IDs remain absent. Empty/control/free-text/unbounded legacy strings quarantine rather than leak.

Tests must cover actual Diff digest arrays, `diff-test`, all-zero/invalid-length/uppercase trace-shaped values, field absence and exact preservation; malformed values cannot silently pass as valid trace context.

These are bounded implementation/compatibility refinements under the current user authority, not Organization/Product semantics or a new external dependency. Reason-bearing producer delivery and source cleanup remain deferred. Independent review must approve the addendum before its source-admission rule is used.

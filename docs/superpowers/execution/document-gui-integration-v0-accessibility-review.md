# Document GUI Integration v0 — Accessibility Review

Date: 2026-10-01 JST
Scope: browser-level review of the approved Mock 1–7 implementation. This records tested criteria and does not claim a complete external WCAG conformance certification.

## Automated checks

| Criterion | Evidence | Result |
|---|---|---|
| Japanese document language, one primary heading, named page landmarks, and named buttons | `apps/document-web/e2e/document-workspace.spec.ts` — responsive and landmark audit | PASS |
| Sampled foreground/background token contrast pairs are at least 4.5:1 | Same test; seven pairs read from computed design tokens | PASS |
| No horizontal overflow at 1280px and 1440px | Same test at both viewport widths | PASS |
| Reduced-motion mode sets spatial motion to 0ms | Same test using `prefers-reduced-motion: reduce` | PASS |
| Keyboard activation opens a document and returning restores list focus and URL context | `keyboard activation keeps list URL context and restores focus after returning` | PASS |
| Publication confirmation supports keyboard focus, Escape, and post-completion focus restoration | `publication workspace requires confirmation and reports success only after the API response`; Jest workspace contract | PASS |
| Mock 1–7 primary screens expose their principal semantic states | Mock journey E2E and reviewed visual snapshots | PASS |

## Manual browser checklist

- Mock 1: list, folder selection, and Context Panel remain distinguishable; keyboard activation and return preserve selection context.
- Mock 2: document identity, lifecycle state, and original-file action are visible in the detail workspace.
- Mock 3: WORKING Version and issued Document Revision are separate and labeled independently.
- Mock 4: native file picker and drop target identify the selected file; upload progress is not fabricated.
- Mock 5: current and target versions, publication method, JST time zone, review acknowledgement, and final confirmation are visible.
- Mock 6: comparison coverage is labeled Partial/Unknown, the unverified range directs the reader to both originals, and the Context Panel is closed.
- Mock 7: effective permissions are shown separately from the editable explicit-policy draft; all five actions are labeled; the internal policy ID is not displayed.

The seven snapshots were reviewed at 1440×900 in the Japanese browser locale. Keyboard, focus, responsive, contrast-token, and reduced-motion checks above are the v0 browser-level evidence for this review.

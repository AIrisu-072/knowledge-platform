# Organization Client v0 — D2 source / interaction review

## 2026-10-02 15:12 UTC — Dialog-edge source repair / hosted proof pending

Parent-verified normal [run37024289977](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37024289977),
job110894867737, failed at15:04:13UTC on remote head
`dd493ea1cc57fd8e64d13204be6b3d9f469e8792`, tree-equivalent to local
`f9402b43f3b977ea646b56c5df71f218ebcf78d6` / tree
`063483f5199023b848ee740f72e134f03fce673a`. The fixed category was
`keyboard-dialog-tab-2-active-body-document-unfocused`: original containment
failed after the second Tab; its immediate diagnostic observed body/unfocused.
This supports focus leaving the two-button dialog at its edge. It does not
identify browser chrome, an OS destination or a background interaction.

The isolated `fix/organization-d2-dialog-keyboard` amendment realizes the already
approved Phase3§8–9 contained modal cycle, consistent with the
[W3C APG modal pattern](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/).
`prototype.js` adds only an open-dialog-local keydown boundary handler: last→first
on Tab and first→last on Shift+Tab. Eligible controls are recomputed per key;
disabled/negative-tabindex/inert/nonrendered/hidden-visibility controls are excluded.
Initial Cancel, ordinary interior DOM order, native showModal/Escape/close and
existing return-focus/draft semantics remain intact. Ctrl/Alt/Meta, other keys,
previously prevented events and closed dialogs are untouched. No global handler
or new business/authorization semantics are added; original owner§50 applies.

Focused DOM RED had10 expected missing-wrap failures; GREEN is38/38 DOM plus
4/4 source, zero skips. Both archetypes cover forward/reverse edges, required
return/adopted fields, Workspace input/disabled Confirm, a single eligible
control, excluded controls, modifiers/closed/outside events, Escape/cancel draft
preservation and trigger/Work fallback. jsdom's explicit visibility/close shims
only exercise handler branches; they do not establish rendering or native keys.

The hosted helper retains every original key/assertion in order and appends
reverse Submit edges, Return textarea order/required validation/input preservation,
and Workspace input/disabled-Confirm edges. It cancels these previews, restores
Evidence, and verifies unchanged normal scenario/draft plus closed dialog before
the ten-state loop. Fixed-stage fault tests had51 expected REDs, then149/149 GREEN.
No waits/retries, screenshots, output payloads or acceptance relaxation were added.
The original five-Tab diagnostic categories and mandatory failed containment stay.

Fresh combined source/DOM/harness verification passes309/309 with zero skips;
all changed JavaScript syntax, safe eight-file source snapshot, documentation
links, exact nine-path scope and whitespace checks pass. The original hosted
keyboard sequence is byte-identical through its last scenario assertion.
Project-wide mise gates were not run: they include the prohibited Rust/build
work; no such full-project result is claimed. No dependencies were installed.

Independent review and a new normal exact-head hosted pass remain **PENDING**.
Actual browser repair, capture/pixels and Phase3 freeze are not claimed; Phase4
has not started. All20 image names,1440 image scope, non-recorded1280/1440 geometry,
workflow/auth/prerequisite/export gates, locks, Phase1/2 and PR43 remain unchanged.
Next exact action: parent reviews this clean immutable amendment, publishes only
a reviewed tree, and obtains new normal hosted proof before considering capture.

---

## Scope and current subject

Independent source/interaction **GO** at local
`d37bb7ab787bb6f9e311f4c8aea16562714af831`, tree
`a7517c356b1809dda8da7faccf9894c73ea2563f`.
[Source-only Draft PR48](https://github.com/AIrisu-072/knowledge-platform/pull/48)
remote `0dcaeea6d1dce37f3833f5c8ec972ebf9368bac1` has that exact tree, parent
D1 `13c1292c806c9179be0a444ef2b4be8234e00bf4`. Current metadata was independently
re-read at13:57UTC. Both remain Draft/open/unmerged. This first D2 publication
contains exactly10 additive source/design files and no capture workflow.

UI specification blob: `6a30859408bedcd57ff3ca117c3ad6bede12eddf`.
Prototype JS blob: `7c0fac5c4da9df794372b1cb01d5c8f8a020e83d`.
Frozen Phase1/2 remain `a2901ccb866fc85b18301db27dd66aa629791201` and
`9f68bf19eb9986eb1c78082704a5d38ff35e35af` respectively.

## Findings and corrections

Initial source candidate `e2a45ed6` / tree `2926c0f0` was **NO-GO** with four
Important issues, despite its original4 source/10 DOM tests passing:

1. Non-first eligible claim switched to the first assigned fixture and used the
   wrong confirmation label. Correction preserves the selected stable ID/label
   when inserting it into the assigned collection, and fences changed targets.
2. Global preview scenario leaked submitted state to a different untouched task.
   Correction keys workflow presentation, draft, decision, Agent state, return
   reason and pinned submission by stable item; task/filter/WorkType transitions
   restore that item's own state without relabeling identity.
3. Context rebuilding removed focused Agent/comparison controls. Correction
   resolves replacement controls by stable ID/action/decision and region, with a
   safe Work-heading fallback. Dialog checks distinguish synchronous/deferred
   close-event emulation from actual browser modal behavior.
4. Modified-decision blank input was replaced by invented text and edited return
   reason was discarded. Correction validates required nonblank input inline,
   retains exact entered content and preserves cancellation/no-mutation behavior.

At `e4687d62` those findings closed, but **NO-GO** remained because submit claimed
one HumanDecision even when none was chosen. `006968cd` pins an immutable copy of
actual submitted draft/Evidence/decision membership; zero decisions stays zero.
Externally selected handed-off fixtures are explicitly seeded, separate from an
interactive submit. An empty eligible queue now displays zero and disables claim.

`006968cd` re-review then found comparison/return module navigation replaced the
workflow scenario, reopening submitted work or removing received return context.
Final `d37bb7ab` separates comparison presentation from workflow state, removes
in-app fixture switching, and preserves pinned read-only content. Outbound return
closes the current work and shows its authorized instruction, never the receiving
actor's new private draft. The receiving returned-draft state remains an explicit
external synthetic fixture, under the depicted recipient's responsibility.

## Final verification and limits

Independent final GO: no remaining Critical/Important findings. Fresh4/4 source,
24/24 qualified-jsdom DOM tests with zero skips, JS syntax and whitespace PASS.
The reviewer also ran six extra cases: both archetypes×submitted/outbound-return/
received-return with repeated comparison, focus fallback and task round trips.
Hostile draft/prompt/adopted/reason text remains escaped. Frozen design blobs
and the ten-file additive scope were verified; reviewed worktree clean.

This is source/interaction proof only. jsdom has no rendering and its dialog shim
cannot prove browser focus trapping, typography,1280/1440 geometry, performance
or actual pixels. The dot cloud browser refused localhost with
ERR_BLOCKED_BY_CLIENT; there was no bypass or user-Mac fallback. The temporary
loopback source server was stopped after that route was confirmed blocked.

The owner approved a separate D2-only hosted20-image/one-day sharing boundary.
The qualification harness is independently developed, still requires exact
integration/privacy/workflow review and normal-head proof, and is not approved
by this source review. Actual capture/artifact provenance/pixel review and Phase3
freeze remain **PENDING**. Phase4 Tauri qualification remains **NOT STARTED**.

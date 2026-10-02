# Organization Client v0 — D2 source / interaction review

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

# Organization D2 — Actual visual review v1 (failed subject)

Status: **NO-GO — two Important presentation findings; corrected pixels pending.**

## Immutable capture provenance

- Remote PR48 head: `bd1f57f49d0a2cee69965302d77c90e791f060d9`
- Captured tree: `c5ec2da69cbe6073e35735f0d45dcb3b5b9765da`; equivalent local source: `826e3186870bc57e3ced87fba458934e8e54523c`
- [Capture run 37028764386](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37028764386), job `110910009723`, attempt `1`
- Artifact `11236900877`; reported expiry `2026-10-03T15:42:16Z` (one-day retention)
- Downloaded ZIP SHA256: `f8bfc7020cf05290dfd467a309db0d9070135f30aa6edf3afd056e2858008417`
- Parent-provided structured artifact/capture receipts supply the identifiers and per-file hashes below. No PNG or raw log is copied into the repository.

## Actual-pixel findings

An independent reviewer inspected all20 original images at their actual full-page dimensions. Capture/harness success does not override this visual NO-GO.

1. **Important1: stale completed-state presentation.** `sales-handed_off.png` shows submitted, immutable membership and next-ready, but the central 「現在」 still says 「内容確認。次は審査担当へ提出」 and 「自分の対応」 still asks for preparation. Both `sales-handed_off.png` and `office-handed_off.png` keep 内容確認 current and 審査 future. Office also retains the prospective 「今回確認すること」 heading. Frozen Product/UX§4 requires actual progress and own next work; selected historic work must not look reopened.
2. **Important2: misleading blocked action.** `sales-blocked.png` and `office-blocked.png` show an enabled-looking blue/white Submit without an adjacent 「提出不可」 explanation. The native disabled property correctly inhibits activation; the defect is presentation, caused by primary/hover CSS overriding disabled appearance. This is not a backend authorization-bypass finding.

The remaining16 images have no mandatory pixel finding. Japanese readability, absence of tofu and clipping passed this review. This does not assert that unpictured interactions or every source body were reviewed.

## Exact image matrix

| Filename | Dimensions | Bytes | SHA256 | Review |
|---|---:|---:|---|---|
| `sales-normal.png` | 1440×1012 | 251715 | `5bdac3531fda0c18a8a4b0ec5e5493e5c070e7c22e0fb1ead2ef3493885c6680` | No mandatory finding |
| `sales-newly_assigned.png` | 1440×1012 | 206208 | `df3ea40c44850a0aee3ef386db00547ea1557636f6eb8ced5789388cb892bbc1` | No mandatory finding |
| `sales-returned.png` | 1440×1107 | 220614 | `f01da9f7698688d521c988d2050199a379703e05d30239004ce0b671e2d71356` | No mandatory finding |
| `sales-working_draft.png` | 1440×1012 | 231995 | `0771e89e123089e928cb3a76b00bc2fcae4b6b90d5643eb763c7f6be68d429cc` | No mandatory finding |
| `sales-handed_off.png` | 1440×907 | 209977 | `7870eb62597310f3638893f0d1dd06d9769759cf2060a8cb793917b16cdf9e58` | Important1 |
| `sales-due_soon.png` | 1440×1012 | 256419 | `9abb27256cc9c93e79e751864b732e6bd731d454937c9d0e1b853293fed74f24` | No mandatory finding |
| `sales-blocked.png` | 1440×1012 | 199708 | `95f83e186e670814fc2a99443f29e0b593be92335c23ab09dca24b8dbffe1957` | Important2 |
| `sales-agent_active.png` | 1440×1012 | 220405 | `6eff07300f97f315be12094dbe3c2a5049b6b03a348ae479ecbcb455ebc94743` | No mandatory finding |
| `sales-evidence_review.png` | 1440×1012 | 256039 | `20c3948efc29363635d6724d768384adec6213bcdeb01b732602c0b8cf121d04` | No mandatory finding |
| `sales-document_compare.png` | 1440×1293 | 234022 | `14b1a7091626b3366c3e958812e33384982a2dfddbf66d0ad8b8d30c1a5134b6` | No mandatory finding |
| `office-normal.png` | 1440×1012 | 258574 | `66df89567e936e053d0a72a1c011f7062de2e6b65670111e8631fef0cfc5a870` | No mandatory finding |
| `office-newly_assigned.png` | 1440×1012 | 213518 | `e92bd9c873b12dffae62b920ab9a72b8c5275f41435f01edad90f641674c4372` | No mandatory finding |
| `office-returned.png` | 1440×1107 | 227732 | `d419bae702feb4c909e6e61e8c7d15e54ef081b704f3de25613322d4b1b74e3d` | No mandatory finding |
| `office-working_draft.png` | 1440×1012 | 238957 | `6dd600b746304c44fceceb07669da111de177e7b216c4117c588a6d0e461e872` | No mandatory finding |
| `office-handed_off.png` | 1440×907 | 217341 | `c5868cc4e7899b93b749fc869dc113fb16f0e8b8994ee5e59190a4778aa32505` | Important1 |
| `office-due_soon.png` | 1440×1012 | 263277 | `16e1af0027cd7910555d41a358170fd3f340c5c4de62aa4bcc4ede812f3778a6` | No mandatory finding |
| `office-blocked.png` | 1440×1012 | 206840 | `e1f0767eadd5798a68aceb9055d276df827901583e71b336203b8b11e29e2336` | Important2 |
| `office-agent_active.png` | 1440×1012 | 228090 | `50f94fde359c807c8fc468510d02468e03dbe28fc00be020c6482636efb1a2e4` | No mandatory finding |
| `office-evidence_review.png` | 1440×1012 | 262899 | `d1f622ffd9f6de724fee3123661d0bf2a0da164c37924ee222ede1d1ec014790` | No mandatory finding |
| `office-document_compare.png` | 1440×1293 | 240218 | `21f77690cfa2ee884283a649ee2d8fc71d3ad2ee9b59d17b2a86e6e30453ce00` | No mandatory finding |

## Evidence limits and successor gate

- These are1440-wide full-page907–1293px images, not1440×900 viewport images. Non-recording1280/1440 geometry, native keyboard and actual font-selection checks are separate hosted evidence.
- Search module body, individual decision outcomes and open dialogs are not pictured in this set. No Tauri, production React, backend/state/auth/persistence or Search-runtime qualification follows from these source-design pixels.
- The captured subject remains failed permanently. A source correction, DOM test or hosted computed-style assertion cannot change that verdict or substitute for a later authorized corrected20-image capture and independent pixel review.
- Phase3 freeze remains pending; Phase4–6 have not started. This receipt grants no capture, upload, new retention, replay or publication authority. Existing owner/head/time/prerequisite/export gates remain unchanged.

See the [source correction receipt](organization-client-v0-ui-review.md) and [qualification status](organization-d2-visual-qualification-status.md) for the pending successor.

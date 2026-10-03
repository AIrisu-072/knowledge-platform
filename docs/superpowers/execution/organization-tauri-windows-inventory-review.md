# Windows inventory review record

Date: 2026-10-03 UTC. First-slice independent review pending. No new metadata,
installation, dependency build or runtime proof yet.

Subject: [design](../specs/2026-10-03-organization-tauri-windows-inventory-design.md),
[plan](../plans/2026-10-03-organization-tauri-windows-inventory.md), and exact
[approval amendment](../../decisions/2026-10-03-organization-tauri-windows-mpl-amendment.md).
Base is actual PR51 remote629822a49657bf447da9061c568dc72558265ce7/treea85096b2753002943acfafd34091760b692b63a6.

Review must cover source authority, no package-code execution, precise role scope,
tool restoration/provenance, Windows host/target distinction, disk budget, preserved
root policy/CI discovery and conditional later gates. No prior review is reused as
approval of these new bytes.

## First-slice independent GO, 2026-10-03 02:25 UTC

Reviewed subject b33c0f2f88135d1e67b0b96b655e383d36f7b2af / tree
265bcb295c330cba1cd102febfb6d8423f84cd0e against actual PR51 base above.
Independent reviewer: **GO for W1–W3 metadata/source/advisory inventory only**;
no remaining Critical/Important finding. Six docs/279 additions, clean worktree,
whitespace and unchanged root source/locks/policy/workflows/frozen designs verified.

The original81b1dfd review required complete host lookup, archive-authoritative
source comparison, pre-extraction member safety and scanner coverage reconciliation.
Corrected f281cd9/14e1b3a/b33c0f2 close these before execution. Exact-version
predicates, combined downloaded/expanded4GiB budget and every-filesystem1536MiB
floor also verified. Official selected tool restoration is conditional on artifact
verification; no dependency build/runtime/Windows-native qualification is granted.
Future fixture bytes require their own independent evidence and CI-routing review.

## Fresh evidence candidate for final review

W1/W2 now complete within the approved data-only slice. Bounded receipts and exact
fixture/lock/validation source are ready for an independent exact-byte review.
Raw data remain externally available for review; no archives, extracted third-party
source, SDK binaries or raw logs are published. Publication packaging amendment
follows later parent direction and does not alter verification/exception scope.

Fresh helper tests18 PASS, original-capture lock/metadata/role/source/scan integrity
PASS. Source verifications cover406 registry archives and22,054 regular files,
including8,552 files in216 selected packages. Root/scoped/all-target scan outcomes
and every remaining native/runtime/host gate are retained in the evidence README.
No local root Rust/application suite ran, because this first slice permits no new
package compilation/runtime; future normal hosted CI remains separately observable.
Final independent review and durable parent publication are still pending.

## Final independent metadata publication GO, 2026-10-03 02:55 UTC

Subject4164683be5109195d9f5d22588888845be0b2ac3/tree9d364531c2788ee1f8c16b086b90181f66437cef:
**GO for publication only**, no Critical/Important finding. Reviewer independently
reran18/18 stdlib tests and original-capture verification, checked406 archives/
22,054 files/216 selected packages, exact roles/scanner coverage and4/5/0 outcomes,
all nine native payloads/notice/package hashes, exact five-exception diff and
preservation of existing source/locks/policy/workflow/capabilities/frozen designs.

All26 paths/958,701 bytes contain bounded receipts and our inert fixture/validation
source, no dependency archive/source, SDK payload or raw log. Cargo/pnpm/assurance/
Docker routing cannot execute the fixture; recursive OSV may truthfully report
inactive advisories, with Rust call analysis still not opted in. A preliminary
publication bundle was briefly untracked inside the worktree, never committed;
it was moved to sibling storage before handoff and is outside the reviewed subject.

The later Runtime agreement discovery is a separate narrow documentation/receipt
amendment requiring its own review. It adds a concrete custom-license STOP; no
metadata result is revoked or broadened into Runtime permission. Native Windows,
terms/security/generated output and actual runtime gates remain unqualified.

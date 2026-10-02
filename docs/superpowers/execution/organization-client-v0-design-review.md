# Organization Client v0 — Independent design review receipts

## Phase1 Product/UX

Initial candidate `83bcd53fea57e1843ab04488154731ada56121ff`, tree
`9298269a6810fd2e075e9a63391fd8ee953d47f5`: independent **GO**, no Critical or
Important finding. Reviewer checked inherited frozen Document contracts,
two-archetype/shared-authority boundaries and original scoped approval.

Editorial correction made OrganizationalUnit/Tauri v2 explicit and repeated the
already-required Phase4 qualification checklist. Exact design blob
`a2901ccb866fc85b18301db27dd66aa629791201`: independent re-review **GO** at12:41UTC.
All52 numbered original requirements, supporting status and additive Active
pointer were also checked. Relative links/whitespace and unchanged prior pointer
content passed. [Phase1 authority](../specs/2026-10-02-organization-client-v0-product-ux-approval.md)
records the freeze; no product/runtime/hosted test claim is inferred.

## Phase2 initial review — NO-GO

Candidate `5dd56998e05fcb74620d3228d9e8f6f0eaeba638`, tree
`6197c20d20e524d62ac804c649846359e48c0f9d`, design blob
`d7eeb997c0f01b0cade56f79bfb5e28c68d85cd9`: four **Important** findings, no
Critical finding. Initial Phase2 freeze is **NO-GO**. Locations below describe
that exact original subject, not later corrected line numbers.

1. Provider-backed private draft references (lines188–238,477–478) cannot prevent
   direct Document reads. Existing `document_history.rs:151–215` uses Document
   rights, not Work assignment; Document/folder ACL ownership is fixed. Promotion
   before failed Work submit could expose content. Required: Work/local ownership
   or proven provider isolation, no pre-submit audience expansion and direct-route
   denial/reassignment/return tests.
2. Local read API (lines381–420) returns file identity but does not bind later
   ranges to a content generation; in-place writes can preserve inode identity.
   Required: bounded principal/device/window/context-bound read handle/generation,
   stable capture, invalidation and mutation counterexamples.
3. Sales continuity (lines143–194,320–322,473–475) lacked independently authorized
   context/progress/history projections between queue and private detail. Required:
   distinct actions, closed permitted fields and count/filter/cursor privacy.
4. AgentExecution (lines285–305,571–583) did not bind actual executor/provider
   identity or intersect requester rights. Existing MCP preflight requires
   `poc/poc-agent`, unlike synthetic `agent-01`. Required: distinct attributable
   identities, server-established bounded dispatch scope and asymmetric/revocation
   denial tests, without weakening fixed-session verification.

These are corrections within the approved meaning, not additional routine owner
approval gates. The parent independently identified the private-provider bypass.

## Phase2 corrected candidate — independent re-review pending

Candidate `19d47c5c0fc555e13435018947d496b08585a6c7`, tree
`585d30f3aaaab580b4d2106a047db9449d1a471c`, design blob
`1c629ca51abae47532aaa60cf624a6c624d3c8c1` contains proposed closures:

- Work-owned private immutable content generations; shared sources explicitly
  remain inputs. Local upload is private before atomic handoff; shared-authority
  storage is distinct from recipient visibility. No Document authority change.
- openRead/readFile/closeRead generation-fenced handles with resource bounds;
  stable capture must prove same-inode safety in Phase4 or report unavailable.
- context.read/progress/history permissions and closed projections, including
  count/filter/cursor and sensitive reason/body exclusions.
- requester, executor and independently verified provider identities; bounded
  dispatch requires their current authorization intersection. No header/body
  principal selection or implicit mapping to fixed `poc-human`/`poc-agent`.

Fresh documentation scope and whitespace checks PASS. Re-review is pending;
Phase2 is not frozen and Phase3 has not started. No implementation, installation,
browser, Rust, database or live Agent work occurred in this review cycle.

## Phase2 final exact-blob review — GO

Full re-review of corrected19d47c5c resolved all four Important findings. One
editorial accuracy correction was requested: unchanged MCP verifies its fixed
Agent session at startup; per-use verification belongs to the new dispatch/
provider adapter. Final commit `245b17ddf39a7ef88a29ce698d57340d79bb7ad6`, tree
`d056e70a8be34c56d062f6f43138d70f8390d392`, design blob
`9f68bf19eb9986eb1c78082704a5d38ff35e35af`: independent **GO** at12:59UTC, no
Critical/Important findings. Exact-object and whitespace checks passed; frozen
Phase1 remained unchanged. No runtime/install/build proof was inferred. Earlier
NO-GO and pending entries are historical; this final receipt closes the design
review only. [Phase2 authority](../specs/2026-10-02-organization-client-v0-domain-api-approval.md)
permits ordered Phase3 work under original§50.

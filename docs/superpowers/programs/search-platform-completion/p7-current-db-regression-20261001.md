# P7 current DB regression — 2026-10-01

Branch `feat/search-platform-completion-core`, baseline HEAD `80a47960d025e4dfdea1eacade28b15d218725ff`, current uncommitted producer/migration snapshot. No production database was used. Cached PostgreSQL 18.6 disposable Testcontainers, offline locked Cargo, debug=0/incremental=0/jobs=2 and serialized Cargo ownership were used.

## W1 producer evidence

`cargo test --offline --locked -p document-repository-postgres --test read_state_transaction -- --test-threads=1` freshly passed 7/7. The existing `mandatory_audit_failure_rolls_back_first_read` injects an INSERT rejection specific to `document.version.read_confirmed`, asserts the service returns error, then reads zero committed read-state rows and zero event rows through the pool outside the failed transaction. `first_explicit_confirmation_is_idempotent_even_under_concurrency` verifies one initial confirmation/event across concurrent calls. Producer is the existing same-transaction INSERT in `src/read_state.rs`.

This closes fresh local execution of the existing business rollback/idempotence regression, without rewriting correct production code merely to produce a new RED. The current test has a different name from the plan's proposed name. Ruling: retain the equivalent existing tested behavior; duplicating it would provide no new assurance and could overlap another active capability's test ownership. Cost if wrong: R09 must still judge the exact class-specific business behavior. R04A-S class policy/legacy decoder and R04A-D sink projection are unimplemented and remain separate W1 gates. Full W1 acceptance is not claimed.

Local log: `/tmp/search-completion-resume-20261001/p7-w1-existing.log`.

## Existing migration evidence and W2 boundary

`cargo test --offline --locked -p search-runtime --test coordination_migration --test source_ownership_migration -- --test-threads=1` freshly passed 3/3 and 6/6. Cases include independent Domain/Search migration ledgers, namespace ownership, explicit legacy proof, receipt bundle version, ownership binding transaction, and tombstone identity guard.

Local log: `/tmp/search-completion-resume-20261001/p7-existing-migrations.log`.

Only Search 0001/0002 currently exist. Required Search 0003, Domain 0010 Audit, Search 0004 inventory and 0005 policy files and their runtime orchestration are absent. The tests do **not** prove W2 Domain0009 → Search0001–0003 → Domain0010 → Search0004 bootstrap order, full checksum/role admission, R03 listener readiness, or R09 restore/acceptance. Those gates remain unimplemented; a successful call to the current single migrator does not substitute for them.

No new commit/PR, hosted exact-head success, merge, deploy or live migration is claimed.

# P6-G04 PostgreSQL exhausted reaper code receipt

- Scope: `PostgresOutboxStore::reap_exhausted` and `postgres_reaper.rs` only, on the uncommitted `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` worktree. G05 runner, real delivery role, Search receipt/publish, complete P6 acceptance, merge and deployment remain separate.
- Contract: frozen `p6-outbox-design-revision-1.md` §3 and `p6-outbox-plan.md` G04. The existing `guard_policy` locks the singleton policy `FOR SHARE`, compares all six fields, and rejects legacy exhausted `attempt_limit IS NULL` rows before the reaper query. The reaper runs inside the same short transaction, validates `limit` in 1..=32, takes one materialized DB clock sample, selects only pinned, undelivered, non-dead rows at/above their row limit with no active lease, orders `last_attempt_at NULLS FIRST,event_id`, and uses `FOR UPDATE OF o SKIP LOCKED`. It sets `dead_lettered_at` and `delivery_unknown_at_limit`, clears lease columns, then commits before returning the count. SQL/connection/commit errors map to `StoreUnknown`.
- Historical `attempt_limit=NULL` and `attempt_count>=policy.max_attempts` rows remain a typed `LegacyExhausted` preflight error, not automatic terminal rows. This makes the SQL's explicit non-NULL predicate equivalent to the frozen recovery matrix after guard success. Reaping preserves event identity, payload, occurrence/availability/last-attempt timestamps, attempt count and row limit. It never writes `delivered_at`.

## Fresh RED / GREEN

| Gate | Observed result |
| --- | --- |
| `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 cargo test --locked -p outbox-delivery --test postgres_reaper -- --test-threads=1 --nocapture` before source edit | 4 failed, 1 child fixture ignored, exit 101. Every failure was the prior `StoreUnknown` placeholder; compile and disposable PostgreSQL migration succeeded. |
| Same command after source edit | 4 passed, 0 failed, 1 child fixture ignored, exit 0. Two independent child processes/pools competed for a final expired claim: counts `[0,1]`, original row retained and one terminal code recorded. Other cases cover six-field policy mismatch and legacy preflight, renewed/unexpired/nonexhausted/terminal row immutability, unclaimed exhausted rows, 32-row cap and locked first-row skip, closed-pool `StoreUnknown`. |
| `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 cargo test --locked -p outbox-delivery --test postgres_claim --test postgres_settle -- --test-threads=1` | G02 claim 3/3 and G03 settle 4/4 passed, exit 0. |
| `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 cargo clippy --locked -p outbox-delivery --all-targets -- -D warnings` | Passed, exit 0. |
| `cargo fmt -p outbox-delivery -- --check` and direct `rustfmt --check --edition 2024` on the two owned files | Passed, exit 0. |
| `cargo fmt --all -- --check` | Could not start workspace check: unrelated in-progress `crates/search-extraction-worker/src/main.rs` does not exist, exit 1. No source was changed to work around this. |

Fixture: cached `postgres:18.6-bookworm`, image `sha256:3725f4e2499eef5134592b3b4ab79a543ed7f8e533b05b5b637af926630f6650`; disposable containers are test-owned. This is functional qualification, not a throughput or large-table lock measurement. Free space after checks: 2.9 GiB; no image pull or global container cleanup.

## Input hashes

| File | SHA-256 |
| --- | --- |
| `crates/outbox-delivery/src/postgres.rs` | `7956b053616763b933f08a3aca58e06a7797ee18acc8d1a9740d88e2ac53dd74` |
| `crates/outbox-delivery/tests/postgres_reaper.rs` | `c21ed7e782b9cda976be80ca8f9f02cad1e3f9c71d314e45c69a4215d0b10c1d` |
| `crates/document-repository-postgres/migrations/0009_outbox_delivery_v0.sql` | `6059108aa478df2b5c9af5417e650ce9d661f74e77236fa0e2b8e02e626cf7bb` |
| `p6-outbox-plan.md` | `29638c077a29aebd007be4a4fd7c5fb7f7bfbf4a785ba0540782f98795811a80` |
| `p6-outbox-freeze.md` | `d9ac0835f8129b09e096486a8e18d8158932ce4ba101917758d94218d442dd9a` |
| `p6-outbox-design-revision-1.md` | `c886a6b0933d6323b0fab7d41f0641e18332c6c74104025442a03609369d5366` |

Next exact action: independent read-only G04 audit of this exact source/test/migration/contract set. G05 runner and G08 role qualification are later tasks; do not infer either from this receipt.

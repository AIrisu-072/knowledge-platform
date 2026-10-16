# `sqlx-postgres 0.9.0` cancelled-begin patch

- Source: published crates.io `sqlx-postgres 0.9.0` archive, SHA-256 `87a2bdd6e83f6b3ea525ca9fee568030508b58355a43d0b2c1674d5f79dcd65e` (matches the root Cargo.lock checksum).
- Upstream: <https://github.com/launchbadge/sqlx> (`sqlx-postgres/`); archive `.cargo_vcs_info.json` records commit `003b698e99e024f3621b8043a2426fde5b741171`.
- License: MIT OR Apache-2.0; upstream `LICENSE-MIT` and `LICENSE-APACHE` are retained here.
- Use: the PostgreSQL driver behind SQLx 0.9 (spec/selection/library-tool-selection-v0.md, DB access) for every production crate.
- Change: `BEGIN_CANCEL.diff` is the complete diff against the archive; only `src/transaction.rs` changes. `PgTransactionManager::begin` counts the queued `BEGIN`/`SAVEPOINT` in the connection's transaction depth before awaiting its reply, and the second rollback guard (`Rollback`) is removed. `sqlx-core`'s `Transaction::begin` already holds the new transaction open while the driver runs, so a dropped future or a failed wait makes its `Drop` queue the rollback for exactly that level.
- Reason: in 0.9.0 the depth was raised only after the reply. A `begin` future dropped while waiting (for example a request cancelled by its client) left two faults:
  - top level: neither guard rolled back (depth was still 0), so the connection went back to the pool with the server-side transaction open. The next plain query ran inside it, and the next `SET TRANSACTION ISOLATION LEVEL REPEATABLE READ` failed with SQLSTATE 25001. Document reads map that to an internal error; this matches the intermittent Organization Finding 503 after a restart (an internal error on the first statement of a Document read, within 0-17 ms);
  - savepoint: both guards rolled back at the enclosing depth, so the enclosing transaction was aborted and its earlier writes were lost while the caller still held it.
- Local verification (2026-10-17, linux/arm64 VM against PostgreSQL 18.6, not hosted CI), the same scenarios as `crates/document-repository-postgres/tests/sqlx_begin_cancellation.rs`:
  - archive: a dropped BEGIN stayed open 20 of 20 times; the snapshot after a dropped pool `begin` failed with 25001; a dropped SAVEPOINT lost the enclosing transaction (1 of 2 rows committed). With random cancellation, 74–82 snapshot failures in 2,000 iterations;
  - patched: no open transaction, no snapshot failure (0 in 3 × 2,000 iterations), and the enclosing transaction kept both rows.
- Packaging: retain the archive contents and omit the package-local Cargo.lock.
- Removal condition: replace this patch with a published sqlx-postgres release whose `begin` rolls back a cancelled BEGIN or SAVEPOINT, after the repository test above passes on it.

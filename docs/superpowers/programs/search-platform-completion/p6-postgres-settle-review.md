# P6-G03 PostgreSQL fenced settle 独立レビュー

- 判定: **G03 限定 GO**。対象は `PostgresOutboxStore::renew`、`settle_success`、`settle_failure` と `postgres_settle.rs` の実 PostgreSQL 試験。G04 reaper、runner、Search publish/receipt、実 role 権限、P6 全体、本番配備の GO ではない。
- 確認: 2026-09-30 JST、`feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` の未コミット作業木。`p6-outbox-freeze.md`、`p6-outbox-design-revision-1.md` §3、`p6-outbox-plan.md` G03、G02 review、policy bounds / policy lock role ruling と照合した。

## G03 の照合

- `crates/outbox-delivery/src/postgres.rs:224-260` の renew、`:263-291` の ack、`:294-347` の fail は、各 statement 内で materialized `clock_timestamp()` を一度採り、event ID・現 token・未 delivered/dead・`lease_expires_at > tick.t` を条件にする。0 行の確定結果だけを `Lost`、SQL/connection/commit error を `StoreUnknown` にする。期限切れまたは旧 token による mutation は試験で拒否されている。
- ack は `delivered_at` と lease の消去だけを行う。fail は DB 行の `attempt_limit` を使い、terminal または上限到達なら `dead_lettered_at` を設定し、その他は DB tick から bounded backoff 後の `available_at` を設定する。両方とも lease を消す。`last_error_code` は `ErrorCode` の固定 allowlist から bind し、原 event/payload、Domain 業務行、Audit 行を更新しない。Search/P7 crate の import もない。
- `postgres.rs:303-310` は整数ミリ秒の backoff を期待 policy の範囲内で検証してから DB に渡す。現行 v0 範囲は 1,000–300,000 ms。`renew` の lease は既存の `validate_claim` を使い、1,000–120,000 ms に限定する。policy の revision/全値一致は起動・claim・reap での条件であり、G03 の settle で policy 行を lock しない実装は Freeze と ruling に整合する。
- `postgres_settle.rs:119-239` は旧 token・expiry ちょうど/超過で3操作が `Lost` かつ行不変、現 token での ack と renew の DB 時刻境界を確認する。`:241-398` は範囲外/端数 backoff の拒否、retry 時刻、terminal、行別 limit 1/8/32 の最終試行 DLQ、lease 消去、payload 保存を確認する。
- `postgres_settle.rs:400-507` の TCP proxy は実 COMMIT を PostgreSQL に転送し、サーバーの committed `ReadyForQuery` を受けた後に client 側へ応答を返さず切断する。呼出側は `StoreUnknown`、別の直接接続による再読は `delivered_at` と lease 消去を確認し、旧 token の再 ack は `Lost`。`:509-551` は閉じた pool で3操作とも `StoreUnknown` と行不変を確認する。proxy 試験が示すのは ack の commit 応答喪失であり、Search publish transaction の未知 commit は後続 S07 の対象。

## 入力と fresh verification

| 対象 | SHA-256 |
|---|---|
| `crates/outbox-delivery/src/postgres.rs` | `00c0dbe8230fb75a4d0a2cd57a8ed20bc7d9856c9ae241eac49b1afbb26f2bc5` |
| `crates/outbox-delivery/tests/postgres_settle.rs` | `8d4dbb47ae1763ff8b9318cac7801b7cf6ededb0ad86e27aa9920fcc13be6bc2` |
| `crates/outbox-delivery/src/model.rs` | `21eed7abd915e1b33280bad16b44f8feb86d4a91a192e2c3c460fa3c0310ad54` |
| `crates/outbox-delivery/src/policy.rs` | `0da03f76bcbff98c92e24d3b80138e56a76eddd080992637f2a9b5a15a5554a3` |
| `crates/document-repository-postgres/migrations/0009_outbox_delivery_v0.sql` | `6059108aa478df2b5c9af5417e650ce9d661f74e77236fa0e2b8e02e626cf7bb` |
| `target/debug/deps/postgres_settle-b47b8b2e20c9128a` | `e193da516989a835a0ab16a288a0386d2c1814ff2af4f8a50dcd4f27847e0a82` |
| `p6-outbox-plan.md` | `29638c077a29aebd007be4a4fd7c5fb7f7bfbf4a785ba0540782f98795811a80` |
| `p6-outbox-freeze.md` | `d9ac0835f8129b09e096486a8e18d8158932ce4ba101917758d94218d442dd9a` |
| `p6-outbox-design-revision-1.md` | `c886a6b0933d6323b0fab7d41f0641e18332c6c74104025442a03609369d5366` |
| `p6-postgres-claim-review.md` | `0c96514e8daf771a829694eb227403b21563dd9965ffe19bab873560892c4169` |
| `p6-policy-bounds-ruling.md` | `e9903fc0ad254303c180e5086bf3fc58de09bfbc1fad311187400c8481b17dfa` |
| `p6-policy-lock-role-ruling.md` | `6ad8420a5e2257c7dc5cfe6a744b496e03c793179ec706e5ce80ff282248ee7d` |

- 主対象の `postgres.rs` / `postgres_settle.rs` と SQL、plan、Freeze、G02 review、二つの ruling、binary の hash は監査開始時と終了時に同一。補助入力の model / policy / design は終了時に上表の hash を確認した。Fresh 実行: `target/debug/deps/postgres_settle-b47b8b2e20c9128a --test-threads=1 --nocapture`、**4 passed / 0 failed / exit 0 / 49.52 s**。fixture は cached `postgres:18.6-bookworm`（image ID `sha256:3725f4e2499eef5134592b3b4ab79a543ed7f8e533b05b5b637af926630f6650`）。試験後に同 image の稼働 container はない。既存の停止済み container は操作していない。
- Fresh `rustfmt --check --edition 2024 crates/outbox-delivery/src/postgres.rs crates/outbox-delivery/tests/postgres_settle.rs` は exit 0。今回 Cargo 再コンパイル・strict Clippy は実行していない。binary 更新時刻は対象 source/test/migration より後だが、時刻と hash だけでは現行ファイルの exact bytes から build された暗号学的証明にはならない。ここでの動的証拠は **記録した cached binary** の fresh 実 DB 実行である。

## 残る境界と次の action

- `postgres.rs:349-352` の G04 `reap_exhausted` は引き続き `StoreUnknown` placeholder。最終 claim 後 crash の DLQ 回収を G03 の即時 fail 試験から推定しない。次は G04 の実装と競合 reaper/expiry 試験。
- この試験は管理者接続であり、delivery role の policy `FOR SHARE` と列限定 grant の実効性は G08/I04 で検証する。Search durable commit 後のみ ack する呼出順、Source/outbox 二重 fence、runner の renew/cancel、実 process recovery は G05 以降と S07 の責務。期限付近の row-lock wait 競合も今回の4試験には含まれない。
- G03 の code review/実 DB 局所判定を親に渡す。統合判定では fresh build/strict Clippy、実 role、G04 と後続 runner/Search 回帰を別途確認する。

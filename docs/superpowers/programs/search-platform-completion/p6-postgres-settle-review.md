<a id="p6-g03-postgresql-fenced-settle-独立レビュー"></a>
# P6-G03 PostgreSQLのフェンス付き結果確定の独立レビュー

[固定された公開原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-postgres-settle-review.md)に対応する意味保存の日本語訳です。原設計の再承認、実装・実行時の適格性検証の追加ではありません。既存ハッシュと実行結果は当時の原文・証拠を指し、訳文のハッシュや現在の検証結果ではありません。以下の状態と次の作業は当時の記録です。[最新の実行状態](../../execution/search-platform-completion-program-status.md)を優先してください。

- 判定: **G03 限定 GO**。対象は `PostgresOutboxStore::renew`、`settle_success`、`settle_failure` と `postgres_settle.rs` の実 PostgreSQL 試験。G04 上限到達行の回収処理、ランナー、Searchの公開・イベント受領記録、実際のロール権限、P6 全体、本番配備の GO ではない。
- 確認: 2026-09-30 JST、`feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` の未コミット作業木。`p6-outbox-freeze.md`、`p6-outbox-design-revision-1.md` §3、`p6-outbox-plan.md` G03、G02 レビュー、ポリシーの上限・下限とロック用ロールの判断と照合した。

## G03 の照合

- `crates/outbox-delivery/src/postgres.rs:224-260` のリース更新、`:263-291` の配送成功確定、`:294-347` の失敗結果の確定は、各 SQL文内で実体化した `clock_timestamp()` を一度採り、イベントID・現トークン・配送済み・デッドレター化済みのどちらでもない・`lease_expires_at > tick.t` を条件にする。0 行の確定結果だけを `Lost`、SQL/接続/コミットエラーを `StoreUnknown` にする。期限切れまたは旧トークンによる変更操作は試験で拒否されている。
- 配送成功確定は `delivered_at` への記録とリースの消去だけを行う。失敗結果の確定は DB 行の `attempt_limit` を使い、終端状態または上限到達なら `dead_lettered_at` を設定し、その他は DBで取得した時刻から上限付き再試行待機後の `available_at` を設定する。両方ともリースを消す。`last_error_code` は `ErrorCode` の固定許可リストからバインドし、原イベント/ペイロード、Domain 業務行、Audit 行を更新しない。Search/P7 クレートのインポートもない。
- `postgres.rs:303-310` は整数ミリ秒の再試行待機を期待ポリシーの範囲内で検証してから DB に渡す。現行 v0 範囲は 1,000–300,000 ms。`renew` のリースは既存の `validate_claim` を使い、1,000–120,000 ms に限定する。ポリシーの改訂番号/全値一致は起動・処理権取得・期限切れ処理の回収での条件であり、G03 の配送結果確定でポリシー行をロックしない実装は設計凍結と判断に整合する。
- `postgres_settle.rs:119-239` は旧トークン・有効期限ちょうど/超過で3操作が `Lost` かつ行不変、現トークンでの配送成功確定とリース更新の DB 時刻境界を確認する。`:241-398` は範囲外/端数再試行待機の拒否、再試行時刻、終端状態、行別試行上限 1/8/32 の最終試行 DLQ、リース消去、ペイロード保存を確認する。
- `postgres_settle.rs:400-507` の TCPプロキシは実 COMMIT を PostgreSQL に転送し、サーバーのコミット済みの `ReadyForQuery` を受けた後にクライアント側へ応答を返さず切断する。呼出側は `StoreUnknown`、別の直接接続による再読は `delivered_at` とリース消去を確認し、旧トークンの再配送成功確定は `Lost`。`:509-551` は閉じた接続プールで3操作とも `StoreUnknown` と行不変を確認する。プロキシ試験が示すのは配送成功確定のコミット応答喪失であり、Search公開トランザクションのコミット結果不明は後続 S07 の対象。

<a id="入力と-fresh-verification"></a>
## 入力と新たな検証

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

- 主対象の `postgres.rs` / `postgres_settle.rs` と SQL、計画、設計凍結、G02 レビュー、二つの判断、バイナリのハッシュは監査開始時と終了時に同一。補助入力のモデル / ポリシー / 設計は終了時に上表のハッシュを確認した。新たな実行: `target/debug/deps/postgres_settle-b47b8b2e20c9128a --test-threads=1 --nocapture`、**4 passed / 0 failed / exit 0 / 49.52 s**。フィクスチャはキャッシュ済みの `postgres:18.6-bookworm`（イメージ ID `sha256:3725f4e2499eef5134592b3b4ab79a543ed7f8e533b05b5b637af926630f6650`）。試験後に同イメージの稼働コンテナはない。既存の停止済みコンテナは操作していない。
- 新たな `rustfmt --check --edition 2024 crates/outbox-delivery/src/postgres.rs crates/outbox-delivery/tests/postgres_settle.rs` は exit 0。今回 Cargo 再コンパイル・厳格なClippy は実行していない。バイナリ更新時刻は対象ソース/テスト/マイグレーションより後だが、時刻とハッシュだけでは現行ファイルの正確なバイト列からビルドされた暗号学的証明にはならない。ここでの動的証拠は **記録したキャッシュ済みバイナリ** の新たな実DB 実行である。

<a id="残る境界と次の-action"></a>
## 残る境界と当時の次の作業

- `postgres.rs:349-352` の G04 `reap_exhausted` は引き続き `StoreUnknown` 未実装の仮置き。最終処理権取得後クラッシュの DLQ 回収を G03 の即時の失敗結果確定試験から推定しない。次は G04 の実装と上限到達行の回収競合・有効期限試験。
- この試験は管理者接続であり、配送用ロールのポリシー `FOR SHARE` と列限定権限付与の実効性は G08/I04 で検証する。Search 永続コミット後のみ配送成功確定する呼出順、Source/outbox 二重フェンス、ランナーのリース更新/キャンセル、実プロセス復旧は G05 以降と S07 の責務。期限付近の行ロック待機競合も今回の4試験には含まれない。
- G03 のコードレビュー/実 DB 局所判定を親に渡す。統合判定では新たなビルド/厳格なClippy、実際のロール、G04 と後続ランナー/Search 回帰を別途確認する。

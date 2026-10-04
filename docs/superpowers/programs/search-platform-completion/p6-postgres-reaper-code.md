<a id="p6-g04-postgresql-exhausted-reaper-code-receipt"></a>
# P6-G04 PostgreSQL 試行上限到達行の回収処理：実装記録

[固定された公開原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-postgres-reaper-code.md)に対応する意味保存の日本語訳です。原設計の再承認、実装・資格の追加ではありません。既存ハッシュは当時の原文・証拠を指し、訳文のハッシュではありません。以下の状態・次の作業は当時の記録であり、現在の実行指示ではありません。[最新の実行状態](../../execution/search-platform-completion-program-status.md)を優先してください。

- 対象範囲は `PostgresOutboxStore::reap_exhausted` と `postgres_reaper.rs` だけです。当時の未commit作業ツリー `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` を対象にしています。G05ランナー、実際の配送ロール、Searchの処理記録・公開、P6全体の受入、マージ、デプロイは別の対象です。
- 契約は、凍結済みの `p6-outbox-design-revision-1.md` §3と `p6-outbox-plan.md` G04です。既存の `guard_policy` は単一のポリシー行を `FOR SHARE` でロックし、6項目すべてを比較したうえで、回収クエリの前に過去の試行上限到達行のうち `attempt_limit IS NULL` の行を拒否します。回収処理は同じ短いトランザクション内で行い、`limit` が1..=32であることを検証し、実体化したDB時刻を1回だけ取得します。固定された行別上限を持ち、未配送・未打切りで、試行回数がその上限以上かつ有効なリースがない行だけを選びます。順序は `last_attempt_at NULLS FIRST,event_id` で、`FOR UPDATE OF o SKIP LOCKED` を使います。`dead_lettered_at` と `delivery_unknown_at_limit` を設定し、リース列を消去して、件数を返す前にcommitします。SQL・接続・commitのエラーは `StoreUnknown` に対応付けます。
- 過去の `attempt_limit=NULL` かつ `attempt_count>=policy.max_attempts` の行は、引き続き型付きの事前検査エラー `LegacyExhausted` です。自動的に終端行へ変更しません。そのため、ガード成功後のSQLの明示的な非NULL条件は、凍結済みの復旧マトリクスと同じ意味になります。回収処理はイベント識別子、payload、発生・利用可能・最終試行の各時刻、試行回数、行別上限を保持し、`delivered_at` を書き込みません。

<a id="fresh-red--green"></a>
## 当時新たに実行したRED / GREEN

| 検証ゲート | 観測結果 |
| --- | --- |
| `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 cargo test --locked -p outbox-delivery --test postgres_reaper -- --test-threads=1 --nocapture`：ソース変更前 | 4件失敗、子フィクスチャ1件をignore、exit 101。すべて以前の `StoreUnknown` 仮実装による失敗で、コンパイルと使い捨てPostgreSQLのmigrationは成功しました。 |
| ソース変更後の同一コマンド | 4件成功、0件失敗、子フィクスチャ1件をignore、exit 0。独立した2つの子プロセス・接続プールが、最後の試行が期限切れになった行について回収を競合実行し、件数は `[0,1]`、元の行を保持したまま終端コードを1件記録しました。他のケースは、6項目のポリシー不一致と過去行の事前検査、更新済み・期限前・上限未到達・終端行の不変性、未取得の上限到達行、32行上限とロックされた先頭行のスキップ、閉じた接続プールでの `StoreUnknown` を確認しました。 |
| `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 cargo test --locked -p outbox-delivery --test postgres_claim --test postgres_settle -- --test-threads=1` | G02 claim 3/3、G03 settle 4/4が成功、exit 0。 |
| `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 cargo clippy --locked -p outbox-delivery --all-targets -- -D warnings` | 成功、exit 0。 |
| `cargo fmt -p outbox-delivery -- --check` と、対象2ファイルへの直接の `rustfmt --check --edition 2024` | 成功、exit 0。 |
| `cargo fmt --all -- --check` | ワークスペース検査を開始できませんでした。無関係の作業中ファイル `crates/search-extraction-worker/src/main.rs` が存在せず、exit 1。この問題を回避するためのソース変更は行っていません。 |

フィクスチャはキャッシュ済みの `postgres:18.6-bookworm`、imageは `sha256:3725f4e2499eef5134592b3b4ab79a543ed7f8e533b05b5b637af926630f6650` です。使い捨てコンテナはテストが所有します。これは機能面の適格性検証であり、スループットや大規模テーブルのロック測定ではありません。確認後の空き容量は2.9 GiBでした。imageの取得や、全体を対象としたコンテナ削除は行っていません。

<a id="input-hashes"></a>
## 入力ハッシュ

| ファイル | SHA-256 |
| --- | --- |
| `crates/outbox-delivery/src/postgres.rs` | `7956b053616763b933f08a3aca58e06a7797ee18acc8d1a9740d88e2ac53dd74` |
| `crates/outbox-delivery/tests/postgres_reaper.rs` | `c21ed7e782b9cda976be80ca8f9f02cad1e3f9c71d314e45c69a4215d0b10c1d` |
| `crates/document-repository-postgres/migrations/0009_outbox_delivery_v0.sql` | `6059108aa478df2b5c9af5417e650ce9d661f74e77236fa0e2b8e02e626cf7bb` |
| `p6-outbox-plan.md` | `29638c077a29aebd007be4a4fd7c5fb7f7bfbf4a785ba0540782f98795811a80` |
| `p6-outbox-freeze.md` | `d9ac0835f8129b09e096486a8e18d8158932ce4ba101917758d94218d442dd9a` |
| `p6-outbox-design-revision-1.md` | `c886a6b0933d6323b0fab7d41f0641e18332c6c74104025442a03609369d5366` |

当時の次の具体的な作業は、この正確なソース・テスト・migration・契約の組に対する、読取専用の独立G04監査です。G05ランナーとG08のロール適格性検証は後続の別タスクであり、この記録からいずれの完了も推定してはいけません。

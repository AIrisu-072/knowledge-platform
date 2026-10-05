# G07の実DB復旧2ケースの結果（2026-10-04）

GitHub Ubuntu上の公式PostgreSQL 18.6と架空データで、凍結計画にある復旧2ケースが成功しました。[実行ログ](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37205082373/job/111444484783)は、各ケースがそれぞれ `1 passed` で実行されたことを示しています。

- 対象PR head：`3d3bfcb2a6881d1a61d724a2d8d15c8d4cd843a3`
- 実際のcheckout：GitHub生成merge commit `3e71b9a3d6c1ea2a4006682ad2d632c1ea67eb4a`
- tree：`97698712021a7e72a3125cc093237d134ef25798`
- `process_recovery.rs`：blob `67dee95513baca7523454d0a17c6fd348718c108`、SHA-256 `95cc5ed0e197f99c25e3ccd1b2ba376b8399c9fb82237a190aacff018d85c302`
- 公式イメージ：`postgres:18.6-bookworm`、digest `sha256:3725f4e2499eef5134592b3b4ab79a543ed7f8e533b05b5b637af926630f6650`

## 確認できた動作

1. `last_claim_kill9_restart_reaps_unknown_once`：13:18:41 UTCにDBからコミット済みの取得を観測した後、子をSIGKILLで終了・回収しました。2つの独立した回収プロセスの件数は `[0, 1]` でした。元イベント、試行数8、結果不明の打切り状態、その後の再取得・再回収なしを検証し、ケースは成功しました。
2. `four_processes_disjoint_claims_and_recover_expired`：4プロセスの8件の有効な取得をDBから観測しました。1プロセスを停止し、その所有者の2件だけを新しいトークン・試行数2で再取得しました。残り6件の不変と、残る所有子プロセスの終了・回収を検証し、ケースは成功しました。

両ケースのリース失効は実時間の経過を待つ方式ではなく、ログに `FORCED_BY_TEST_SQL` と明示した試験SQLによります。一時DBは既存の専用フィクスチャの管理下で使用しています。以前拒否されたローカルソケットを再実行した結果ではありません。

## 検証の限界と次の作業

純粋テスト2件では実装前のREDと実装後のGREENを取得しました。実DBでは、接続済み候補の2ケースが成功したことを実証しています。過去に未接続版の実DB REDを取得したとは記録せず、後日の負の対照を時系列上の初回REDへ読み替えません。元の全資格試験、G08、P6、Search全体の完了を主張するものではありません。

同headの全体CIでは、G07、policy、macOS両variant、portability、containerが成功しました。Rustの静的検査とテストは既存の `outbox_delivery::observe` 欠落で失敗しました。履歴秘密情報検査は921 commitsで検出0、cargo-denyとOSVは成功し、その後のworkflow lintが未引用の `HEAD^{tree}` に対するShellCheck SC1083で失敗しました。

引用を加える1行だけを独立レビューし、head `6441f7d4f0a244a952f72f7855e7201d5601f722`、tree `1b76634675a4fd0917a245470ab7c44487a2e96d` として公開しました。復旧ソースは同一です。この新headのCI結果は別に確認します。次は、凍結済みG08の有限ラベル・トレース検証・配送ロール権限境界を最小実装し、必要なDB試験を同じ合成ホスト環境で行います。

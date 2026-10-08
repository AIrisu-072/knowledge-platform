# 文書管理残タスク 2026-10-08：状況

状態: IN PROGRESS / 未統合 / 未配備。main基点 `2a37d35cd228344f98e0194de16d5336fa786e3c`、branch `feat/document-remaining-20261008`。

## 現在の境界

元checkoutにはactive/statusの未コミット変更があり、そのまま保全した。別コピー `/Users/airisu/Documents/Codex/2026-10-08/task/knowledge-platform` で最新mainを取得した。repositoryに `.agents/skills` は存在しない。ユーザーのsession保存領域は参照していない。

最新mainのCI [37703216247](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37703216247) は14 jobs成功。PR106はmerged、merge commitは上記main。これは新変更の資格ではない。

項目2は既存GUI接続を確認し、指定版が未取得の場合に現行版へfallbackする不備を回帰試験化した。項目3は既存getDocumentRevision接続を実装中。focused REDは2 suites、4失敗/67成功/71件。新変更のGREEN/独立レビュー/hosted CIはこれから。

項目4と6は親のクラウドワークスペース担当。Macはschedulerの合成6利用者接続案を調査したが製品未編集。PR85元commit `53f08ac7537f9378b4d90c4810350471ff0d0f50` のdetect.rs/worker_contract.rsのみを一時抽出して `cargo test -p document-semantic-inspection-worker --test worker_contract` 13/13成功、fmt成功を確認し、差分は戻した。検索変更は取り込んでいない。

項目1/7/8/9の設計案を作成し確認待ち。製品実装はしていない。既読は現行の詳細正常表示契機を保持する。対象PC、認証/TLS、本番directory/権限、実backup/restoreは未実施・確認待ち。運用追補と大量測定計画を作成したが、新規1000件以上の実測はない。

## 検証環境

Rust 1.98.1、固定Node24、pnpm12.4.1/lockfileで依存準備。通常pnpm shimはENOEXECのためJest/tscはNode24から直接実行する。空き約8 GiB、Docker socket権限拒否。実Linux DB/DSI/scheduler、対象PC、実復元、公式PDF大量投入の合格は記録しない。

次のexact action: 項目2/3のGREEN試験・独立レビューを完了し、同一Draft PRを作成して正確なhead CIを確認。統合直前に親のscheduler/DSI変更との順序・最新mainを再確認する。

# 文書管理残タスク 2026-10-08：状況

状態: IN PROGRESS / 未統合 / 未配備。main基点 `2a37d35cd228344f98e0194de16d5336fa786e3c`、branch `feat/document-remaining-20261008`。

保存先は同一の **[Draft PR108](https://github.com/AIrisu-072/knowledge-platform/pull/108)**。製品commit `ee8b98f04fa012481cafa755add8db86c00666ef` / tree `3837d48cd4bbe1fd3cfcf52d763a82f2af016f45` で独立レビューGOを再確認した。本節のPR記録追記は文書のみで、製品source/試験/画像は変えない。最新保存headとhosted資格はPRのChecksと説明を参照し、読み直した正確なheadで判定する。

PR作成直後の製品headでは通常CI `37731617010`、DSI PoC `37731617017`、Sandbox `37731617158` の開始を確認した。これは完了・合格の記録ではなく、文書追記後のheadへ成功を転用しない。hostedの最終結果はheadを変えないPR説明にも記録する。統合順とmain統合・統合後CIはクラウド親が担当する。

## 現在の境界

元checkoutにはactive/statusの未コミット変更があり、そのまま保全した。別コピー `/Users/airisu/Documents/Codex/2026-10-08/task/knowledge-platform` で最新mainを取得した。repositoryに `.agents/skills` は存在しない。ユーザーのsession保存領域は参照していない。

最新mainのCI [37703216247](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37703216247) は14 jobs成功。PR106はmerged、merge commitは上記main。これは新変更の資格ではない。

項目2は既存ボタンのpublished/authoring用途では過去版を取得できない不備を確認し、明示history表示と既存ページ列へ接続した。指定版が未取得の場合の現行版へのfallbackを廃止した。項目3は既存getDocumentRevisionへ接続し、保存時snapshot/actor/reasonを表示する。focused REDは2 suites、4失敗/67成功/71件。

製品の独立レビューはGO。文書間選択残留、非表示/復帰、古い応答、履歴認可失効、一覧/単体詳細の再読取り、UNKNOWN固定要求再送、2拒否案内のaccessible nameを確認した。全体試験で見つかった通常選択保持/移動fixture/共有拒否の6失敗は修正し、最終の固定Node24.21.0 Jestは **74 suites / 1740件成功 / 0失敗 / 0skip**。関連4 suites191件、回帰3 suites181件、独立関連8 suites370件＋最終34件も成功。中断した編集中runや旧失敗runを資格へ含めない。

schema生成照合、TypeScript、Webpack production build、diff checkが成功。ビルドは3件の性能advisory（main JS709 KiB / entrypoint738 KiB）。承認済みの容量SLOとして扱わない。

最終ChromiumはAPI mockの機能試験 **8/8成功**。旧版detailはhistory以外404として試験し、取下げの対象URI/body/current保持を確認した。キーボード/画面遷移/1280・1440幅/reduced motionも検査。8080は別OrbStack作業が使用していたため停止せず、同じpreviewのPORTだけを8181にした一時configで再検証し、作成した一時filesは削除した。**golden画像比較は `--ignore-snapshots` により未資格**。追加ボタンを含む2画像を目視し、見出し/UUID/メタデータの折返し、確認対象/理由欄に重なり・欠けがないことを確認した。[改訂詳細](evidence/document-remaining-20261008/revision-detail.png)、[過去版取下げ確認](evidence/document-remaining-20261008/past-withdrawal.png)は合成データの画面証拠でありgolden更新ではない。

項目4と6は親のクラウドワークスペース担当。Macはschedulerの合成6利用者接続案を調査したが製品未編集。PR85元commit `53f08ac7537f9378b4d90c4810350471ff0d0f50` のdetect.rs/worker_contract.rsのみを一時抽出して `cargo test -p document-semantic-inspection-worker --test worker_contract` 13/13成功、fmt成功を確認し、差分は戻した。検索変更は取り込んでいない。

項目1/7/8/9の設計案を作成し確認待ち。製品実装はしていない。既読は現行の詳細正常表示契機を保持する。対象PC、認証/TLS、本番directory/権限、実backup/restoreは未実施・確認待ち。運用追補と大量測定計画を作成したが、新規1000件以上の実測はない。

## 検証環境

Rust 1.98.1、固定Node24.21.0、pnpm12.4.1/lockfileで依存準備。通常pnpm shimはENOEXECのためJest/tscは固定Nodeから直接実行する。観測した空きは約8 GiB→試験準備後約6 GiB、Docker socket権限拒否。新機能の実Linux DB/server結合、DSI/scheduler、対象PC、実復元、公式PDF大量投入の合格は記録しない。

次のexact action: PR108の現在headとChecksを読み直し、通常CI・DSI/Sandboxの結果をPR説明へ記録する。親が統合直前にscheduler/DSI変更との順序・最新mainを再確認する。main統合と統合後CIはまだ実施していない。原本構成/初回複数/主体検索/viewerは設計確認後に続行する。

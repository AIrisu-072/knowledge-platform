# Organization Browser PoC 最小縦断実装

## 承認と範囲

2026-10-04 05:20:11 UTC の所有者回答「先行してください。許可します」は、Tauri実機検証を後に回し、ブラウザ版で模擬ユーザー2名の「タスク一覧→文書参照→提出→次担当への引継ぎ」を先行する限定順序変更を承認した。

Phase1–3の凍結設計を維持する。基点はPR49 `0860e34ebd2c2353f6528423a6ebf52cfa70751d`。レビュー済みpreview変更 `b4c221a6` の9ファイルのみ再利用する。Tauri資格取得完了、Phase6全体完了、production利用は主張しない。ライセンス・既知脆弱性・既知実行拒否・公開境界は変更しない。

## 最小成果

- 起動時固定の `sales-01` と `office-01`。リクエストから利用者を選ばない
- 同じWorkItemをcontext/queueの2投影で表示し、タスク詳細と自分のprivate draftを扱う
- 既存Document機能と認可を再利用し、shared inputを参照する。Documentをprivate draftの保管先にしない
- private draft保存、OCC付き提出、変更されないHandoffSnapshot、次担当のready taskをPostgreSQLで保存する
- Browserのnative/Workspace機能不足、Search/Agent未実装は明示する

## 実装順序

1. `work-domain` / `work-application` / `work-repository-postgres` を追加。Work専用schemaと別migration ledger、trusted actor、private disclosure、原子的提出とoperation replayを実装する。文書のmigration/モデルは変更しない
2. `work-api-http` にsession、tasks、private draft、claim、submit、handoff、operation recoveryの最小APIを追加。RFC9457と明示的な安全なコード、requestのidentity拒否を維持する
3. Organization composition rootを追加。既存Document rootを小さくfactory化し、Document単独2profileはそのままに同じtrusted identityで既存routerを再利用する
4. 既存Reactアプリへタスク起点と2投影を追加。既存Document feature/transportを再利用し、Pending・Conflict・Unknown outcomeは明示する。提出は確認後にserverの結果で更新する
5. 狭い独立レビュー、最小テストと既存Document GUI回帰・buildを実行し、実行済み/未実行を分けて引き渡す。新たなCI監督frameworkは作らない

## 最小検証

- 別利用者のlistと既知IDによるprivate draft取得拒否
- 同じoperationIdの提出再送が同じsnapshot/次taskを返し、別digestを拒否
- 古いrevisionの更新/提出拒否と、原子的提出後の再読込整合
- 次担当が提出済み内容を受領し、元のprivate draftを直接読めない
- 既存Document GUI回帰、型検査、production build

実PostgreSQL/HTTP/browser実行が環境拒否で止まる場合は、その証拠を報告して停止する。他socket・別環境へ迂回しない。純粋テストだけで実runtime合格とはしない。

## 最小実runtime確認の追加準備

初回commitの後に、既存Document CI jobの後段でOrganization専用の短いrunnerを実行するsourceを準備する。Document runner本体や資格条件は変更せず、既存harnessのprocess/readiness/cleanup関数を再利用する。別owned PostgreSQL containerと2つの合成DBを用い、既存ignored transaction試験、実Reactの2名journey、同じDB/storageでのserver再起動後確認だけを追加する。画像・trace・videoのcapture/uploadは追加しない。

個別のhosted実行許可が確認されるまではsource準備と純粋/静的検査だけを行う。既知ローカル拒否の再試行も、自動runtimeを起動するpushも行わない。

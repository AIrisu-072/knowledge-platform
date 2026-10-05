# Organization Browser PoC — タスク内Document参照の状況

## 2026-10-05 00:46 UTC — 実原本の観測方法を補正、再受入待ち

- [PR65](https://github.com/AIrisu-072/knowledge-platform/pull/65) 初head `989e1d7c` の[実runtime](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37247650348/job/111568621515)は実DB/transaction/初期化、公開改訂/原本一覧表示と原本APIのHTTP200まで成功したが、Playwright response.body()のbytes長と原本metadataの比較に失敗した。再起動/復元/通常shutdownは未実行、finally cleanupは成功、公開artifact0。機能全体の実受入は未完了
- 製品は既存fetch Response.blob()を渡すが、固定PlaywrightはCDPの文字列responseをUTF-8へ再符号化する。保存原本と観測bytesが異なり得る純粋反例を確認した。ただし初回runの実charsetや差分数値は未取得であり、今回の原因と断定しない
- 既存Document実試験と同じ、実Downloadのprivate一時bytesをreadFileして確認する方法へ限定補正する。元のHTTP200、metadata/合成fixtureのsize、SHA-256とfilenameの期待値は維持し、不一致を許容しない。finallyで削除し、既存context終了時の削除も残す
- 一時download許可はjourneyの2contextだけ。persistenceの拒否設定、画像/trace/video off、artifact0を維持する。製品UI/API/Bridge/ACL/fixturebytesは変更せず、恒久設定や外部送信は加えない
- 補正後GUI240件/20 suites、pure runner17件、両型/schema、Playwrightの各1件収集成功。新exact-headで実Downloadとmetadataの一致を再確認するまで未受入とする

次のexact action: この2pathの限定独立レビュー後に新headを公開し、同じhosted条件の実操作・再起動・cleanupと全CIを終端まで確認する。以下は各時点の記録。

---

## 2026-10-05 00:18 UTC — 選択保持とfocusを補正し再レビュー中

- 独立レビューのImportant: モジュール往復で入力文書の選択が先頭へ戻る点を、既存のprincipal/責任/Task/attempt別タブ内状態へDocument IDだけ保持する最小修正で対応した。providerメタデータや原本bytesは保持しない。参照削除時に別文書へ自動変更しない
- Minor: 原本取得が401/403/404で拒否され、消える領域にfocusが残る場合だけ再読込へ戻す。利用者が別の入力へ移したfocusは奪わない
- 各REDを確認後、固定sourceでcontrollerの全GUI240件/20 suitesが成功。実装担当も両型/schema/build・runner17件を再確認した。途中REDと並行したcontrollerの1失敗を保存し、最終commitでは再現しないことを確認済み
- 次は修正差分の独立再レビューと、日本語Draft/同条件hosted実受入。実DB/画面の新経路はまだ未受入である

---

## 2026-10-05 00:07 UTC — 最小UI接続と純粋回帰完了、hosted未受入

- 営業/事務のContext Surfaceで、既存入力文書の現在の公開改訂・Version・AUTHORITATIVE原本の名前/種類/サイズを表示し、明示的に取得する。既存typed APIとBinaryTransportBridgeを再利用し、Documentの認可/版/download処理は変更していない
- provider拒否・公開改訂無し・途中失敗では古いメタデータを残さない。Task/identity/入力文書の切替と再読込で遅延応答を分離し、古い取得結果からダウンロードを始めない。閲覧/取得でTaskを変更せず、文案/判断の入力を保持する
- 両archetypeと非開示/取得のRED→GREEN。controllerの最終GUI230件/20 suites、application/runtime型、schema freshness、production build成功。既存Webpack advisory3件。既存typed client/BinaryTransportBridgeの純粋6件も成功
- 同じ2名の既存journeyへ、画面から取得した実原本responseのサイズ/hashとTask不変を確認する短い手順を加えた。Playwrightはjourney/persistence各1件のcollection-only。取得データをartifactへ追加せず、既存restart/cleanupを保持する
- 新API/DB/migration/依存lock/CI workflowの変更はない。ローカルDB/socket/listener/browserは起動していない。新sourceの実DB/Chromium結果はまだ取得していない

次のexact action: 最終treeの限定独立レビュー、日本語Draft公開、同じ一時DB/固定2名/画像無しのexact-head実受入と全CIの終端確認。以下は準備時点の履歴。

---

## 2026-10-04 23:54 UTC — Frozenの既存Document読取りを接続

基点は[PR64](https://github.com/AIrisu-072/knowledge-platform/pull/64) exact `2519be299e626bb11609daaf0b83a5a2cb8a9450` / tree `b22b75ef0564fb047d6fc6cb995ee2ddf3686cc7`。独立branch `feat/organization-document-context-slice`。

- 基点の保留/再開は[通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37244049146)・[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37244049143)・[Sandbox](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37244049195)成功。[実runtime](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37244049146/job/111558294416)の実DB/transaction/模擬2名/両HTTP server再起動/復元/shutdown/cleanup成功、Rust894件・GUI202件/19 suites、公開artifact0
- [Frozen UI §5・§8](../specs/2026-10-02-organization-client-v0-ui-design.md)と[Domain §6–7](../specs/2026-10-02-organization-client-v0-domain-api-design.md)に沿い、既存Task入力Documentの公開改訂・Version・原本一覧/明示取得をContext Surfaceへ接続する。別の公開済み文書をTaskへ追加する機能は含めない
- 現状はDocument詳細へのリンクのみ。既存typed client/API/現在provider認可/BinaryTransportBridgeを再利用する。公開改訂と内容版を区別し、原本をinline実行せずダウンロードする。既存の文書詳細/比較リンクも保持する
- 新しいAPI/DB migration/依存/権限/添付書込/本番Identityは不要。Taskの文案保存や提出を暗黙実行しない。未保存入力は既存タブ内状態に保持する。別のSearch/main統合変更は含めない
- 純粋試験は両archetype、非公開/権限喪失時の非開示、選択/identity切替と遅延応答、取得の重複・失敗・文案保持を対象にする。同じ固定2名・一時DB・画像無しの既存journeyへ短い原本読取り確認を追加する

現状: 独立worktreeの基点GUI202件/19 suitesとschema freshness成功。機能のTDDを開始した。新sourceの実DB/browser結果は未取得。ローカルDB/socket/listener/browserは実行しない。

次のexact action: RED→最小接続→回帰/限定独立レビュー→日本語Draft→exact-head hosted実操作・全CIの終端確認。

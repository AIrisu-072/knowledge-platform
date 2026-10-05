# 文書の初回登録を画面操作へ追加する

## 意図と追加範囲

2026-10-05の所有者指示「GUI v0ではまずは画面操作のみで大半のことができるように内部処理があるものを実装して使えるようにして欲しい。新規作成する必要があるものは後回しでいいです。」に基づく、既存処理の最小GUI追加である。初回登録を最優先とする。

基点はmain `d20f2c1c47b5dcbebdfd1901e3ce7d433fbd58eb`。凍結[GUI設計](../specs/2026-09-30-document-gui-integration-v0-design.md)の七画面へ登録導線を追加するが、承認済み原本・業務意味・認可・Document/Version/Revisionの規則は変更しない。初回登録後はWORKINGであり、公開操作は別に行う。実装内部の細部を所有者の個別事前承認と記録しない。

## 小さい画面変更

- 作業: 一覧から登録先フォルダー・文書名・原本ファイル1件を確認して、下書き文書を登録する。単発の明示操作とし、一括登録は含めない
- 登録先: 検索で選択中のフォルダー。全体表示では取得済みルートIDを使い、IDを固定しない。子フォルダーの可否はそのフォルダーのchildren応答にある現在capabilityを使う
- 判断: 初期metadataは既存APIどおり空object。本文抽出・公開・権限変更を自動実行しない。成功したらauthoring詳細へ移動する
- 失敗影響: 初回createはサーバーがIDを生成し、operationIdを受け付けない。二重押下を抑止し、結果不明後は再POSTしない。既存Problemに回復用3 IDsがある場合だけ既存GETで結果を照合する。通信断・回復404を未作成と断定しない
- 操作: 既存React/Query/React Ariaのフォーム、キーボード移動、エラー表示を再利用する。取消・戻る・再表示・遅延応答で別操作へ成功を適用しない。入力ファイルやタイトルを永続化しない

## TDDと受入

1. 既存API adapterにBinaryTransportBridge.createDocumentと生成recoverDocumentCreationを接続する。型/API/backend/依存は新設しない
2. 純粋GUI試験で初回登録、権限なし、空入力、サイズ超過、二重押下、取消と再表示、選択先変更、遅延応答、結果不明後の再POST禁止、回復GET成功/失敗を先にREDにし、最小実装でGREENにする
3. 既存hosted実Document journeyへGUIでの初回登録→下書き読取→原本一致→既存公開→再起動後の保持を追加する。既存使い捨てPostgreSQL/合成profile/Chromiumだけを使用し、新しい検証基盤や画像・trace・videoの公開を追加しない
4. 型検査・全GUI回帰・build・独立レビュー後に日本語Draftを公開し、exact-head全CIと実runtimeを確認する。main mergeは親担当、実サーバー反映は所有者が手動で行う

ローカルDB/socket/listener/browserは起動しない。Search/Audit/Toolboxの停止作業、外部モデル、本番Identity、Organization添付、新backendは対象外。WORKING更新/rebase、予約取消/撤回/公開終了、metadata/folder管理、既読確認は後続の小さい追加として扱う。

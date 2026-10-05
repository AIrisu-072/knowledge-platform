# 文書共通属性3項目の編集GUI

## 意図と境界

所有者の「内部処理があるものを画面操作から使えるようにする」指示による、既存T5の小さい画面追加。公開main `e9c7f7737f1ddac676c3880475b83fb3cd7135c7` / tree `9085ce0db85857df50ad7d80b6db71d16477df40` を基点とする。[凍結GUI設計](../specs/2026-09-30-document-gui-integration-v0-design.md)、[管理基本設計](../specs/2026-09-28-document-management-basics-v0-design.md)、両承認済み実装計画を保持する。内部の細部を所有者の個別事前承認とは記録しない。

概要から文書種別 `document_type`、所管部署 `owning_department`、カテゴリ `category` と理由を編集する。明示した削除チェックだけをunsetとし、空文字・空白は値として扱う。改行を含む現値を保持し、未変更値をPATCHへコピーしない。extensions、legacy aliases、任意の独自キーは表示と保持だけにする。概要・一覧の主要表示も正本snake_caseへ揃える。移行・削除・alias正規化はしない。

既存 `patchDocumentMetadata` / `PATCH /v1/documents/{documentId}/metadata` を使用する。UUIDv7 operationId、表示時のDocument revision、set/unset、trim後1〜1024 UTF-8 bytesで制御文字を含まない理由を送る。正式Revision番号はクライアントで作らない。実変更でOCCが増え、公開済みなら正式Minorが増える。初回公開前は正式Revisionなし、no-opは期待revision一致を必要とし、changed:falseをそのまま表示する。Version・原本・既読は変更しない。

updateMetadata capabilityだけで導線を決め、mutationの現在認可を正本とする。予約中は無効化、開いた後の競合・権限失効はAPI拒否として扱う。T10後の通常GETには文書が出ないため、終了文書のGUI対応は主張しない。

## 操作と回復

- 概要の「メタデータを編集」→現値3項目・削除チェック・変更理由→明示「保存する」/「キャンセル」
- 二重送信を同期的に抑止。未送信の入力は取消/URL変更で破棄する
- 未確定操作は既存QueryClientの寿命に限るメモリーへ保持し、別文書や戻る/進むの古い応答を新しい入力へ反映しない。永続draft保存を新設しない
- 結果不明は操作IDと完全に同じpayloadを固定し、利用者の「同じ内容で再送」だけで確認する。GET値一致を成功証明にせず、存在しない管理操作回復GETを追加しない。ページ再読込/タブ終了では再送材料を失うことを表示する
- 成功後は文書・一覧・版・改訂・履歴・比較を無効化して再取得する。PATCH応答にmetadataはない

## TDDと検証計画

1. 純粋GUI試験を先にREDにする。3キー差分、明示削除、空と不在、未知キー保持、reasonのbyte/control境界、no-op、二重送信、未知結果の固定再送、409/権限失効/予約競合、取消・再表示・URL別文書・Back/Forward・遅延応答を確認する
2. API adapterと独立フォーム/メモリー内操作状態を最小実装しGREENにする。型・全GUI・production buildを確認する
3. 既存hosted受入に専用specを追加し、固定合成2profiles・PostgreSQL18.6・Chromium・再起動・所有cleanupで実確認できるようにする。専用specの画像/trace/videoは全てoff、追加metadata値をログ/artifactへ出さない。collection-onlyと純粋診断だけをローカルで行う
4. 独立レビューを受け、修正を再検証し、正確なcommit/treeと日本語packetを親へ渡す。公開・merge・CI実行は親の担当。実サーバーへの適用は所有者が手動で行う

ローカルDB/socket/listener/browser、停止中Search/Audit/Toolbox、新backend/認可/依存、既読GUIやFolder修正は対象外。

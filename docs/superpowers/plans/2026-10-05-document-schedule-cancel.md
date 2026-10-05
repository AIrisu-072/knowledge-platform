# 公開予約取消GUIの小さい実装計画

基点はmain `e9c7f7737f1ddac676c3880475b83fb3cd7135c7`、独立branch `feat/document-schedule-cancel-20261005`。[正規read補修](../specs/2026-10-05-document-schedule-cancel-read-amendment.md)の範囲だけを扱う。

1. 既存VersionDetailのnullable currentPublicationScheduleIdを契約/DTO/Repository試験で先にREDにする。既存snapshot内の小さい照会、DTO、OpenAPI exampleを追加し、正規 `pnpm api:generate` でSDK型を生成する
2. 確認付き取消、戻る、capability/identity不在、二重送信、409・実行race・権限失効、同intent再送、遅延応答、Back/Forwardを純粋GUI試験でRED→GREENにする。既存mutationと認可条件は変更しない
3. 同じhosted Ubuntu/PostgreSQL18.6/固定合成profile/Chromium受入に、独立文書のGUI予約→取消→再予約→再取消、ID切替、履歴保持、再起動後の状態を追加する。専用specのtop-levelでscreenshot/trace/videoをoffにし、既存private診断と外部artifactゼロの境界を保持する
4. API/生成型/純粋GUI全回帰/build/collection-only、独立レビュー後、日本語Draft用packetを親へ渡す。親がexact-head CI/同hosted受入とmain統合順を管理する。実サーバー反映は所有者が手動で行う

ローカルDB/socket/listener/browserは起動しない。Rustは専用target、jobs2、排他で実行する。新依存・新lock・新検証基盤・内部Playwright設定を追加しない。Search/Audit/Toolboxの停止作業と既存公開枝へ変更を広げない。

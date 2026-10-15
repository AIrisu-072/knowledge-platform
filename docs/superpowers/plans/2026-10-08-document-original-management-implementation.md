# 原本構成編集・初回複数原本登録の実装計画

2026-10-08 06:23:37 UTC、親が提示した方針へのユーザー回答「この方針で進めてください」を親から受領した。範囲は作業版の原本追加・削除・並替（最後の原本は削除不可、公開版と履歴を保持）と初回複数原本の原子的作成（部分登録しない）。設計原本は[原本構成設計](../specs/2026-10-08-document-original-management.md)。同じ範囲の承認を再要求しない。既読・実アカウント・実環境の設定は変更しない。

基点 `6440e026`、branch `feat/document-originals-20261008`、専用worktree `knowledge-platform-originals`。親のPR108資格確認と統合調整を待ち、local commitのみ。push/PR/mergeは親が指示する。

1. Rust/backend担当: 先に複数初回作成、途中失敗、unknown、回復全件照合、legacy回帰のREDを作る。初回command/service/authoritative model/PostgreSQL transaction、multipart parser/DTOを実装する。単原本形式も同経路で保持し、path+ordinalの形式互換規則を変更しない。
2. 作業版GUI担当: 先に追加/除外/取消/上下移動/最終原本禁止/path検査/unknown固定再送のREDを作る。application helperとManifestFormのみを拡張する。構成変更時だけordinal再採番し、公開・履歴へ直接書かない。
3. 統合担当: API schemaと正式生成SDK、binary初回複数multipart、receipt、初回GUIを実装する。単原本APIの互換性を保ち、result unknownでは再POSTを禁止する。receiptには生成IDsのみを保存する。
4. 合流後にfocused tests→全GUI/型/API contract→Rust/DB検証を行う。日本語操作文書と実受入を同じ機能変更へ追加する。独立レビュー、exact-head CI、main統合後CIは親の調整範囲。未実施の検証を合格と記録しない。

共有ファイル: 統合担当が `spec/api/openapi.yaml`、`spec/api/schemas/document/commands.yaml`、`models.yaml`、`packages/document-api-client/src/generated/*`、`binary-transport.ts`、`apps/document-web/src/api/document-api.ts` を所有する。viewerのdownload上限変更は別worktreeで行い、このbranchでは初回upload部分だけを触る。Rust担当はschema/generatedを変更しない。DocumentDetailPageは触らない。

Verificationは実行ログに記録し、REDとGREENを区別する。Rustは既存shared cacheとjobs2を使い、重複targetを作らない。実DB/browserを起動する場合は共有環境と衝突しないことを先に確認する。データは合成fixtureのみ。

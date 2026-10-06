# 正式改訂の続き表示を既存readへ接続する小計画

## 目的・基点・範囲

既存内部処理を通常GUIから使う所有者の指示と、2026-10-06のPR単位main統合方針に沿う。PR88統合main `ea40684833ebcdf945636300b7b22741725fc80c` を基点に、Frozen GUI §11の正式改訂keyset paginationを「版」と「新旧比較」へ配線する。既存APIのread + readHistory、pageSize=100、opaque nextCursor、新しい順を維持する。新backend/OpenAPI/生成SDK/認可projectionは不要。

Version一覧のhistory用途やT10参照、既読記録、ACL editor、比較結果そのもののpaginationは追加しない。正式改訂とWORKINGコンテンツ版を混同せず、読める名称から権限を推測しない。実装・試験・日本語手順を1本のPRにまとめ、main mergeは親、実サーバー反映は所有者の手動とする。

## 共通制約

- 既存typed API `listDocumentRevisions(documentId, cursor?)` とTanStack Query/React部品を使う。100件の取得数は変更しない。自動全件取得・新しい総件数API・汎用read基盤・新依存を作らない。
- 現在GETの認可を維持する。続き表示は成功したreadのnextCursorに基づき、Versionのcapabilityを履歴権限へ読み替えない。
- 既存document-revisions prefixのinvalidate/resetを維持し、query-backed/WeakMapの固定操作、blob、Organization providerは消さない。
- ローカルはNode24.21.0/pnpm12.4.1固定lockの純粋GUI・型/build・collectionだけ。DB/socket/browser/Cargoは実行せず、実受入は既存hostedを使う。画像/trace/video・golden・skip・timeoutを変更しない。
- Search/Audit/Toolboxの停止作業、新backend/権限モデル、100件fixtureのための大規模検証基盤を追加しない。

## Task 1: 正式改訂ページ列と比較対象の保持

対象: `apps/document-web/src/routes/DocumentDetailPage.tsx`、必要なら小さい専用application/component、`apps/document-web/test/document-revision-pagination.test.tsx` と既存workspace/API試験の関係部分。

- [x] API facadeがcursorを既存pageSize=100とそのまま送ること、GUIが101件目を追加できないこと、明示比較IDが未取得時に先頭2件へ置換されることを反例で確認する。通常の実route DOMと実API wrapperを試す。
- [x] 単ページcacheと混ぜないページ列keyを既存document-revisions prefix配下へ設ける。「版」と「新旧比較」で同じ取得済み列を使い、重複Revision IDを除く。サーバーの順序とnullable次cursorを保持する。
- [x] 「正式改訂をさらに表示」、追加中、続きの再試行、「最初から読み直す」を明示する。初回loading/errorと追加errorを区別し、続きの一時失敗で既取得行を空にしない。二重clickで重複GET/ページ欠落を起こさない。
- [x] 初回read拒否・認可失効・cursor staleを「正式改訂なし」「比較対象が少ない」へ変換しない。明示的な先頭再読取で回復し、古いcursorを別要求へ無言で流用しない。認可拒否後の過去の一覧/比較結果を現在認可済みの表示として残さない。
- [x] Document変更、tab/URL往復、遅延応答、再読取、metadata/公開/移動による既存read resetとの境界を検査する。未確定mutation要求を捨てず、現在navigationを元へ戻さない。
- [x] URL/操作で明示したbaseRevisionId/targetRevisionIdは、未取得・stale時にも保持する。先頭2件へfallbackして別比較を送らない。未取得の選択であることを表示し、取得済み列で両IDを解決できるまで比較GET/古い比較結果表示を止める。追加読取や利用者の明示選択で回復する。ID未指定時の従来の既定候補は維持する。
- [x] 100+1、終端、重複、追加失敗/再試行、stale/失効、初回error、遅延別文書、同batch連打、古い比較ID保持/回復を実DOMでRED→GREEN確認する。focused後に全GUI・schema/型/build、独立spec/品質レビューを行う。

## Task 2: 既存実受入と日本語手順

対象: 既存 `metadata-editor.spec.ts` のjourney/persistenceと関係する既存純粋guard・有限診断、日本語GUI手順/status/Active。新case/fixture/runnerは追加しない。

- [x] 既存の正式改訂1.0/1.1を通常「版」「新旧比較」で読む受入sourceを最小追加し、正式改訂・コンテンツ版の区別、現在GET、終端で続きが不要なこと、明示した比較IDのtab/再読取保持を検査する。実行資格は下記hosted gateで別に取得する。
- [x] 既存HTTPの2改訂/pageSize=1のcursor受入を回帰として維持する。GUIのページサイズは試験だけの都合で変えず、100件超の大量metadata mutationを追加しない。
- [x] 既存2 HTTP再起動後にも同じ正式改訂を通常画面で再表示する受入sourceを追加し、元の全snapshot/原本/既読保持・文書移動replay・metadata/未読/日時検査を維持する。新body/ID等を公開診断へ足さない。
- [x] 純粋反例→GREEN、runtime型・MCP compile・collection18+5を確認する。
- [x] 独立組合せレビューと所見の限定補修・再レビューを確認する。
- [ ] 同head既存CI/実受入/cleanup/artifact0で公開資格を得る。
- [x] 100件を超える実GUIの追加ページは今回の実例未試験と明記する。DOM100+1と既存backend cursor試験を、その実GUI資格へ読み替えない。macOS golden/画像・本番Identity/対象PC/PGプロセス再起動等の既存未資格を保持する。
- [x] 導入pinは現在の合格版のままの場合、文書移動と今回の続き表示が未収録である範囲を明示し、結果だけの別PRを作らない。

## レビュー観点

比較対象IDが未取得時に別対象へ変わらないこと、失効/初回errorが空一覧にならないこと、追加の一時errorで取得済みページを失わないこと、遅延/連打/再開始による混在がないこと、既存操作cacheを壊さないことをTask 1の反例に含める。実100件境界の未資格は隠さず、巨大fixtureを作って範囲を拡大しない。

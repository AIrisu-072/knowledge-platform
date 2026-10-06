# 文書イベント履歴の続きを既存APIへ接続する小計画

## 2026-10-06 20:20 UTC：PR92統合mainへの通常追従

旧公開head `81be7976` と最新main `f431c374` の履歴を保持したmergeを、同じPR91へ保存する。最新mainの閲覧専用コンテンツ版履歴と、本機能のイベント履歴のread/cursor/拒否・明示再読取・未確定操作保持を共存させる。新しい業務意味やbackendを加えない。

- [x] 競合5filesを両UI/全受入assertion/公開記録を保持して解消し、通常詳細拒否と両履歴の独立した再読取の共存反例を検証する
- [x] 全GUI/schema/型/build、既存runtime型/純粋検査/収集18+5、独立組合せreviewを行う
- [ ] 同PRへ両parentのmerge commitを保存し、新head自身の通常既存CIとhostedを確認する

旧headの正式runtime stdout未取得と匿名公式GET403停止は[実行状況](../execution/document-history-pagination-status.md)に保持する。旧CIの再実行やログ取得の代替を目的とせず、異なるsourceの組合せを検証する。新headの成功を旧headの実行証拠へ転用しない。

## 目的と承認範囲

既存内部処理を通常GUIから使う所有者の指示と、機能単位のPRをmainへ取り込む方針に従う。PR90統合main `c2b68850aa022ac77ff180e5010c2197f949036d` を基点に、既存「履歴」タブへ100件単位の続き表示を接続する。Document management basics §12とFrozen GUIの既存History projectionを使い、新backend/OpenAPI/生成SDK・認可・イベントの意味は変更しない。

対象は文書の業務イベント履歴であり、完全な監査調査機能ではない。版一覧のpurpose追加、公開終了文書の新しい履歴route、Search/Audit/Toolboxの停止作業、新しい基盤・依存・大量fixtureは追加しない。実装・試験・日本語手順を同じ1本のPRへ収める。main mergeは親、実サーバー反映は所有者の手動とする。

## 固定するUI/API契約

- 既存 `getDocumentHistory(documentId, cursor?)` のpageSize=100とopaque cursorを維持し、明示操作で続きだけ取得する。空/短いページから終端を推測せず、nextCursorで判断する。
- 現在APIのread + readHistory認可を毎要求で維持する。能力表示や普通の文書読取成功から履歴権限を推測しない。
- このcursorは認可情報へ結び付いたoffset型であり、ページを跨ぐ完全snapshotを保証しない。同時更新に伴う重複を `(sourceKind, sourceKey)` の組で除き、サーバーの順序と最初に取得した行を保持する。総件数・完全取得済みとは表示しない。最新状態は明示先頭再読取で確認する。
- 「変更履歴をさらに表示」「変更履歴の続きを再試行」「変更履歴を最初から読み直す」を設ける。初回失敗を空履歴に変換しない。通信断/retryable 5xxの追加失敗は既取得行を保持し、同じcursorだけを明示再試行する。
- 401/403/404、stale/validation/非再試行可能エラーは旧行を隠して明示先頭再読取へ止める。履歴だけの拒否を普通の文書読取や他画面の拒否と推測しない。文書detailの現在認可拒否では履歴ページも失効させ、自動retry成功で旧履歴を復活させない。
- 単ページcacheと混ぜない専用keyを既存document-history prefix配下へ置く。追加失敗後の既存invalidate、move/reset、明示再読取、tab/文書/route往復で旧cursorや新先頭/旧tailを混在させない。取消済み旧readの遅延成功/拒否を現在文脈へ注入しない。
- 既存の日時不明・実行者不明・由来/provenance表示を保持する。過去の正確な時刻や人物を推測しない。未確定mutation・固定要求・blob・Organization provider、URLの版/比較IDやreturnToを維持する。

## Task 1: 履歴ページ列と拒否後の明示再読取

- [x] 実route/API wrapperで100+1、特殊文字cursor、空/短い非終端、終端、重複source組、未知日時/人物/provenanceの反例を確認する。
- [x] 必要最小の履歴専用hook/controlと既存HistoryTabへ接続する。認可拒否の保持とdocument detail拒否からの失効を追加する。
- [x] 連打、通信断/503同cursor再試行、auth/stale/validation停止、初回エラー、遅延/取消/別文書、tab/route往復、metadata等の既存invalidate/move/reset、未確定操作保持をTDDで確認する。
- [x] focused→全GUI・schema/型/build、独立仕様/品質レビューと所見の限定修正を行う。既存正式改訂・比較ページ列の挙動を回帰確認する。

## Task 2: 既存実受入と日本語手順

- [x] 既存regulation journeyの履歴読取と既存persistenceへ、通常GET/100/cursorなし、描画行/由来、終端、明示再読取とHTTP再起動後の確認を最小追加する。元のsnapshot/原本/既読・比較等を保持する。受入sourceの実行資格は後段のhosted gateで別に確認する。
- [x] 新case/fixture/runner/診断fieldを追加しない。実GUI101件目のfixtureはないため、DOM100+1と実通常read/再読取を別資格とし、実GUI100件超は未資格と明記する。他APIのpageSize=1試験を履歴continuationの証明へ流用しない。
- [x] 固定Node24.21.0/pnpm12.4.1/lockの純粋試験・runtime型・MCP compile・収集18+5、独立組合せレビューを行う。ローカルDB/socket/browser/Cargoは実行しない。
- [ ] 日本語GUI手順と収録版の限界を同PRへ含め、exact-headの既存hosted CI/実受入・cleanup/artifact0で確認する。画像/macOS golden・本番Identity/TLS・対象PC導入・backup/restore・PGプロセス再起動等の既存未資格を保持する。

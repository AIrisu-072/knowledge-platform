# 比較結果の続きを既存APIへ接続する小計画

## 目的と承認範囲

所有者の「内部処理があるものを通常GUIから操作できるようにする」指示と、機能単位のPRをmainへ統合する方針に従う。基点はPR89統合main `e249fb8da91549115d1371c05959e3219dbfde1c`。Frozen GUI §16.4/§17の既存display projectionとcursorを使い、比較結果の51件目以降を通常画面から追加表示する。比較の意味・認可・監査・backend/OpenAPI/生成SDKは変更しない。

正式改訂一覧の100件単位の読取はPR89の実装を維持する。本機能は比較結果の本文差分と未比較範囲が対象。新しい検証基盤・依存・大量fixture・自動全件取得・画像/golden更新・Search/Audit/Toolbox作業を追加しない。GUI・試験・日本語手順を同じ1本のPRへ含め、main mergeは親、実サーバー反映は所有者が手動で行う。

## 固定するUI/API契約

- 全ページでdocument/baseRevision/targetRevisionとprojection=display、pageSize=50を固定し、直前の成功応答のopaque cursorだけを送る。cursorを解析・生成せず、nullまたは省略を終端とする。
- `displayItems` と `unverifiedRegions` の両方をページ順で追加する。空のdisplayItemsや50件未満から終端を推測しない。metadata差分は全体で1回だけ表示する。
- base/target、projection/pageSize、metadata両digest/判定/changes、本文status/verdict/coverage/resultDigestが先頭と整合するページだけを結合する。audit IDは各POSTで変わり得るので一致条件へ含めない。不一致時は新headerと旧内容を混ぜず明示的な先頭再読取へ止める。
- 初回loading/error、追加中、同じ続きの再試行、「比較結果を最初から読み直す」を区別する。通信断・retryable 5xxの追加失敗は取得済み内容を保持し、同じcursorで明示再試行する。401/403/404は既存の文書全pair拒否処理へ接続し、stale/validation/比較入力不一致は旧結果を隠して先頭再読取へ止める。
- 単ページcacheとInfiniteDataを混在させず、既存revision-comparison prefix配下の専用keyに置く。pair/文書変更、認可失効、改訂一覧の再読取、metadata/公開/移動の既存read resetで旧ページとcursorを破棄する。未確定mutation・blob・Organization providerを保持する。
- 中断済み旧要求の遅延成功/拒否、同時連打、再読取中の旧後続ページを検査する。GUIの能力表示を現在API認可の代替にせず、比較のcoverageや未比較範囲を捏造しない。

## Task 1: 比較ページ列と通常GUI

- [x] 実route DOMと既存API wrapperで、50+1、未比較範囲のみの続き、空display+cursor、終端をREDにする。
- [x] 必要最小の比較専用hook/controlsと既存比較描画へ接続する。基準・対象・metadata・本文判定は一貫したものを1回表示する。
- [x] 連打、同cursor再試行、stale/validation/auth拒否、header不一致、異なるaudit ID、明示pair/URL保持、遅延別pair/別文書/再読取、既存prefix resetと操作保持をTDDで確認する。
- [x] focused→全GUI・schema/型/build、独立仕様/品質レビュー、所見の限定修正を行う。

## Task 2: 既存実受入と日本語手順

- [x] 既存metadata2改訂と既存通常内容比較の受入sourceを最小拡張し、通常比較・終端・metadata一回表示・比較再読取と固定pair、HTTP再起動後の読取を検査する。元のsnapshot/既読/replay・正式改訂readを維持する。実行資格は後段のhosted gateで別に確認する。
- [x] 既存fixtureを再利用し、新case/runner/大量比較fixtureを作らない。実GUI50件超は未資格と明記し、合成DOM50+1と既存HTTP pageSize=1の回帰試験を実GUI資格へ読み替えない。
- [x] 固定Node24.21.0/pnpm12.4.1/lockの純粋試験・runtime型・既存有限診断・収集を確認し、独立組合せレビューを行う。ローカルDB/socket/browser/Cargoは実行しない。
- [x] main e249自身の資格を確認後、導入手順のpinを同PR内でe249へ追従し、今回の比較続き表示がそのpinに未収録であることを明記する。結果だけの別PRを作らない。
- [ ] 日本語Draftと同headの既存hosted CI/実受入・cleanup/artifact0で資格を確認する。画像/macOS golden、本番Identity/TLS、対象PC、backup/restore、PGプロセス再起動等の既存未資格を保持する。

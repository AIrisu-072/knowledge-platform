# 文書イベント履歴の続き表示：実行状況

## 2026-10-06 18:19 UTC

- 基点main `c2b68850aa022ac77ff180e5010c2197f949036d` / tree `d9c30aed0858a6c5f798a00df544e46f9f93b5e5`、branch `feat/document-history-pagination-20261006`。[小計画](../plans/2026-10-06-document-history-pagination.md)に従う。
- PR90 head `7e652d9a` は[CI37502459832](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37502459832)で全13jobs・全18checks中15成功/3既存skip、GUI1212/49、Document18+5/Agent9/Org8、DB36/Folder4、HTTP再起動/cleanup/artifact0を確認して統合。main自身の[push CI37505782574](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37505782574)も18:04:36 UTCに成功終端し、独立して全13jobs/13checks、GUI1212/49、Rust1849/既存skip10＋21＋7/既存skip1、DB36/Folder4、実受入/再起動/cleanup/artifact0を確認した。metadata persistence個別行の有限出力省略とDocument cleanupは同source/合格条件との対応推論、Organization cleanupは直接出力と区別する。
- 基点mainのHistoryTabは先頭100件のみを読み、失敗時にもcache行を併記していた。文書detail拒否が失効させる対象も正式改訂/比較readだけだった。今回の候補で履歴ページ列と現在認可拒否・明示再読取・既存操作保持を整えた。版のhistory-purposeや新backendは追加しない。
- 小計画 `feaecf8f` に沿うTask 1 `989e72a4` は5filesの履歴専用hook/control・既存route・実DOM/API試験を追加した。33反例REDから100+1・tuple重複・未知情報・追加失敗/拒否/往復・invalidate/reset・取消済みreadを実装。最終履歴44件を含むfocused144/6、全GUI1256/51、schema/型/build・diff検査が成功した。既存build性能warning3件は保持する。
- 通常のread resetで既知の拒否を消さないため、行やcursorを含まないerrorだけの小さいQueryClient markerを別keyへ保持する。購読で即座に旧行を隠し、同QueryClient内のGCでは消さず、明示履歴再読取で解除する。tab/route退出はpage列だけを破棄する。他文書/新readへの取消済み拒否注入、既存固定操作/blobへの干渉は反例で確認した。
- Task 2 `c42258e0` は既存regulation journey/persistenceの2filesだけを拡張した。GUIのGET100/cursorなし・行順/由来/時刻/実行者・終端・明示再読取、保存snapshot/本人既読/原本の保持を確認するsourceを追加し、従来の比較受入を保持した。safe66・runtime型・MCP compile・収集18+5・diff検査は成功。新case/fixture/runner/診断fieldや検証基盤はない。
- Task 1は固定5hash一致と独立144/6成功で仕様/品質GO、Task 2も限定source review GO。日本語4docsを含む全11pathsの組合せもGOとなり、基点の挙動を現在形で書いていたstatusの1行は訂正・再確認済み、未解決所見なし。今回の新hosted資格は未取得であり、source確認や収集を実browser合格へ読み替えない。
- 固定lockのoffline確認は供給元metadata不足で失敗し、通常の公式registry取得も実行環境の接続中断・未決定の承認取消で終了した。process不在を確認し、同一環境・同一呼出しの通常復旧を1回だけ行った結果、exit0、供給元検査724項目合格、687依存再利用・download0、lock不変を確認した。検査無効化や別経路は使っていない。
- 次のexact action: 候補を固定して同機能の日本語Draftへ保存し、exact-head既存hostedを確認する。実結果は同PR本文へ記録し、結果だけの別PRを作らない。main mergeは親が行う。
- cursorはoffset型で完全snapshotを保証しない。実GUI履歴100件超、比較50件超/正式改訂100件超、画像/macOS golden、本番Identity/TLS、対象PC導入、backup/restore、PostgreSQLプロセス再起動は未資格。WORKING全headers喪失、Home focus、PR82/83未解明失敗も保持する。
- 実サーバー反映は所有者手動。通常の既存CIを維持し、停止されたSearch/Audit/Toolbox作業や新しい検証基盤は追加しない。

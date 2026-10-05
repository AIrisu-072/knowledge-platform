# 公開予約取消GUIの状況

## 2026-10-05 05:06 UTC — ローカル検証と独立レビュー完了、実受入待ち

- 初期基点: main `e9c7f7737f1ddac676c3880475b83fb3cd7135c7` / tree `9085ce0db85857df50ad7d80b6db71d16477df40`
- branch: `feat/document-schedule-cancel-20261005`。初期実装 `101e51609` を保持し、受入済み取下げ・公開終了のmain `5d262557f1d59ab10db2eeb5d808c19382b2414a` と両履歴を保持して統合する。metadata PR71へは変更しない
- [設計追加](../specs/2026-10-05-document-schedule-cancel-read-amendment.md)と[計画](../plans/2026-10-05-document-schedule-cancel.md)に限定。既存readへnullable予約IDを追加し、既存取消mutationを確認GUIから呼ぶ。新table/migration、WORKING更新/rebase、capability規則、認可方式は変更しない
- 正規IDは認可済みsnapshotの同Document/Version・PENDING・予定時刻一致だけ。Historyから推測せず、過去予約のIDを現在予約へ置換しない。結果不明intentはタブ内QueryClientへ固定して保持する

## REDと修正

- baseline純粋GUI259件成功。初期取消GUI14件はUI不在の11件RED、既存非表示3件PASS。最小実装後14件GREEN
- 追加同期cache試験で、再読取開始とsubmitの同tick競合をRED確認。送信直前に現在query状態とID/capability/revisionを照合して修正
- [独立レビュー](document-schedule-cancel-review.md)で成功後focus消失をRED再現し、同contextだけのfocus復帰を追加してGREEN。取消23件と独立反例5件成功
- API契約の新nullable field試験とHTTP DTO明示null試験は実装前RED確認後GREEN

## 最終ローカル検証

- 統合後の全GUI: 302件 / 23 suites成功
- application/client/runtime型、schema freshness、production build、MCP受入bundle build成功。Webpackの既存系統advisory3件は継続
- API契約16件、SDK6件、生成の再実行byte一致成功。API lintは成功し、既存Search localhost warning1件は継続
- HTTP純粋6件、repository純粋1件成功。変更対象のHTTP/repository strict Clippy成功
- HTTP `read_http` とrepository `document_history_projection` のDB統合2targetはcompile成功のみ。DB試験実行は未実施
- 既存runtimeのローカル許可pure範囲116件成功。listenerを使うharness/response-lossのsuiteは実行しない
- collection-only: journey15件、persistence3件。専用取消specは各phase1件。collectionを実browserの成功とは扱わない

## hostedへ渡す検証

専用合成文書でGUI予約→戻る（送信なし）→取消→再予約→再取消、現在予約ID切替、既存正式改訂/原本/公開版/履歴の保持と同2profilesのread一致、両HTTP server再起動後状態を確認する。新specのtop-levelで画像・trace・videoをoffにし、同owned領域の非添付snapshotだけを使用する。標準private raw診断と外部の有限診断、公開artifactゼロの境界は維持する。

ローカルDB/socket/listener/browserは未実行。hosted受入/新exact-head CI/公開/merge/実サーバー反映は未実行。新mainとの組合せの資格を旧headから付け替えない。

次の操作: 日本語Draft packetを親へ渡す。親が公開後の全CIと同hosted受入を確認し、main統合順を管理する。実サーバー反映は所有者が手動で行う。

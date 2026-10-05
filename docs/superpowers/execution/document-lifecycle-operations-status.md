# 版の取下げ・文書の公開終了GUIの状況

## 2026-10-05 03:50 UTC — 最小GUI実装・純粋検証・限定レビュー完了

- 状態: ローカル候補。実DB/browser・exact-head CIは未実行。PR公開とCI監視は親担当へ引き渡す
- 公開基点: main `e9c7f7737f1ddac676c3880475b83fb3cd7135c7`。受入済みPR69をmergeした同一treeである。ローカル作業基点: 初回登録GUI `2470d8cdb5386320301e001dc81cb3daff3ea28f` / tree `9085ce0db85857df50ad7d80b6db71d16477df40`。branch `feat/document-lifecycle-operations-20261005`。基点は公開PR69 `5b14bfe3` と同一treeだが、本候補へそのCI資格を付け替えない
- 追補: [小さい計画](../plans/2026-10-05-document-lifecycle-operations.md)、[画面操作手順](../../operations/document-gui-v0.md)。GUI/Versioning/公開終了の凍結意味と承認原本は変更しない。所有者の既存API GUI化の指示の範囲であり、UI内部細部の個別事前承認は記録しない

## 実装した範囲

- 通常公開画面の「版・改訂」から、現在capabilityが許可する現行公開版の取下げと、文書全体の公開終了を行う
- 理由必須・影響説明・確認取消・同期二重送信抑止。取下げ後の復帰/nullはAPI結果から表示する。公開終了後のread404でも成功を保持し、古い詳細の操作は表示しない
- 送信時の対象・UUIDv7操作ID・期待revision・理由を固定。未確認の要求をQueryClient内で保持し、同じアプリ起動中の戻る/進む・文書/版切替後も同内容だけを再送する。結果不明後の403などを未実行の証明に使わない
- 401/403/404/409/422の確定した拒否は最新状態を確認してからやり直す。成功後は関連するdetail/版/改訂/履歴/原本/一覧/比較queryを無効化する
- 未解決要求が一つでもあれば、画面に依存せず再読込/タブ終了に警告する。要求はメモリだけで、理由や本文を永続保存しない。警告を無視した再読込後の自動復旧は保証しない
- backend/API・依存・lock・migration・認可・Search/Audit/Toolbox停止作業は変更していない。手動導入手順は新しい操作説明へのリンクのみで、固定導入SHAを変更しない

## 検証の記録

- 基準: 既存GUI259件/21 suites PASS
- 最初の新GUI試験: 新操作ボタンが未実装の16件RED、既存の権限非表示1件PASS。その後17件GREEN
- 独立レビュー指摘: Aの結果不明後にBへ移動すると再読込警告が消える経路を追加RED1で確認し、QueryClient全体の監視へ修正。authoring readはWORKINGだけという実API境界もmockに反映し計19件GREEN
- 既存概要画面へ空の操作パネルが混入する追加RED1を修正し、新GUI20件GREEN
- application/runtime型、schema freshness、production build、既存MCP受入bundle buildはPASS。Webpack既存のperformance advisory3件を保持する
- 既存hosted runnerへ専用journey2件・再起動1件を追加。固定2合成profile、使い捨てPostgreSQL18.6、既存Chromium、所有container cleanupを使用する。既存fixture・既存persistence stateを変更せず、原本hash、版/改訂、操作台帳IDを別の同run状態へ保存する。新ケースの画像・trace・videoは成功/失敗とも記録しない
- runtime診断は有限source/固定7段階だけを追加し、追加RED1→診断17件GREEN。診断＋記録off配線26件PASS。実行は型とcollection-onlyに限定し、ローカルDB/socket/listener/browserを起動していない

最終sourceの全GUI279件/22 suites、journey14件/6filesとpersistence2件/2filesのcollection-only、diff検査はPASS。限定独立再レビューはGOでCritical/Important/Minor残件0。純粋検証・collectionを実browserや実DBの成功として扱わない。

前提の[PR69](https://github.com/AIrisu-072/knowledge-platform/pull/69)は公開head `5b14bfe3e3d3ea87016ddb0df1021fbc7aa9458f` で、[通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37259298276)、[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37259298332)、[Sandbox](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37259298350)がSUCCESS。通常CI13jobs、Document実runtime12+1、Organization実runtimeと所有cleanupが成功し、画像artifactは0。親担当から2026-10-05に受領したこの前提証拠は、本候補の新しい取下げ/公開終了受入を証明しない。main merge済みだが、実サーバー導入は所有者が別途行う。

## 後回しと次の操作

予約取消は必要なpublishOperationIdを現行readが返さない。History.sourceKeyから推測しない。過去版の取下げ選択にはhistory purpose/readHistoryが必要なので、今回のreadを暗黙変更せず別の明示UI追補とする。WORKING更新/rebaseは初回current=null、capability不整合、PUT全manifest置換での追加原本/rendition保持を整理してから実装する。新backendは追加しない。

次のexact action: commit/treeと日本語PR packetを親へ渡す。ローカル枝も同一treeのmain `e9c7f773` へ基点を整合済みである。親がDraft公開後に、そのexact headの全適用CIと既存hosted実受入を終端まで確認する。main mergeは親担当、実サーバーへの反映は所有者の手動操作とする。

# 履歴一覧から旧版・原本・イベントを開く小計画

## 目的と承認範囲

所有者の既存API GUI化指示に従い、PR91/92統合main `933d3b0f894e610496022defae8e494b16de39ea` を基点にする。通常詳細が取得できない公開終了・全版取下げ後の文書も、既存 `view=history` 一覧から選択し、既存history-purposeで旧版・原本・イベントを閲覧する一機能である。

通常Document detailにはhistory用途が無いため、published/authoring詳細へ権限や選択の意味を混ぜない。既存一覧内の閲覧専用領域を使い、新しいDocument detail API、復元/再公開/過去版取下げ、Folder ACL、公開前比較、明示既読、主体追加を実装しない。正式改訂の単体詳細も今回の到達性に必須ではないため含めない。新たな検証基盤・大量fixture・画像比較は追加せず、GUI・試験・日本語手順を同機能1PRへまとめる。

## 固定するUIとread契約

- メインナビゲーションの「文書履歴」から既存 `/documents?view=history` を開く。履歴viewに不適合な公開一覧の未読条件等を持ち込まず、既存の文字/属性/日時/Folder条件・sort/pageを再利用する。履歴には現在の文書も含まれ、「終了済みだけ」の新filterは作らない。
- 現在の成功した一覧応答で確認できた文書を明示選択し、領域「選択した文書の履歴」に代表版情報と既存のコンテンツ版履歴・全AUTHORITATIVE原本・イベント履歴を表示する。未選択/未取得/消失した明示IDを先頭行へfallbackしない。通常detailを取りに行かず、正常な履歴readから通常detailの許可を推測しない。
- HistoryDocumentのtitleは一覧の代表版名、metadataは現在Documentの値である。選択Versionのtitle/metadataと正式Revision snapshotは別であり、過去のDocument metadataを捏造しない。serverの`ended`と版のPUBLISHED/WITHDRAWNを別に表示し、PUBLISHEDを現在公開中と推測しない。契約に無いcurrentVersionIdや、nullのfolderId/nameを補完しない。
- 履歴対象の編集・移動・公開・取下げ・既読操作をこの領域へ接続しない。返却されたmutation capabilityから管理操作へ自動昇格しない。原本は選択版のdownload capabilityとAUTHORITATIVE行だけを使い、毎回のAPI再認可・監査・返却IDを維持する。
- 通常詳細のT10後404は、history権限の恒久的な拒否ではない。既存refusal markerと古い成功dataを無言で復活させず、一覧での明示選択/各履歴の明示再読取からfresh history APIだけで再開する。履歴自体の401/403/404は既知の拒否として保持し、自動retry200やview往復で旧cacheへ戻らない。
- 一覧が読取拒否/再取得中/別条件・別pageになった場合や、選択行が現在の応答に無い場合は古いsummary/詳細/原本操作を止める。終了/取下げを0件や404から推測しない。既存APIのcursorと拒否/一時失敗/明示再読取をそのまま使う。
- 条件・文書選択・close・別navigation・中断後の遅いreadやBlobを新しい対象へ注入/保存しない。新しい入口と戻りは同じアプリ内での遷移とし、未確定operation store・upload Blob・Organization contextを保持する。公開/編集の既存経路を壊さず、read resetだけを行う。

## Task 1: 通常入口と閲覧専用panel

- [x] 実route DOM/APIの既存harnessで、通常navigation→history一覧→終了/取下げ後文書の明示選択→旧版/原本/イベントをREDにする。normal detail/変更APIが呼ばれないことを確認する。
- [x] AppShellの入口、既存DocumentHomeのhistory分岐、小さい閲覧専用panelを追加し、既存content/eventのhook・表示・読取controlsを必要最小限で再利用する。汎用基盤を新設しない。
- [x] 現在/過去metadata・ended/state・null・明示ID不在を区別し、一覧拒否/自動再試行/遅延/条件やpage変更/close/再表示/ダウンロード取消/未確定操作保持をTDDで確認する。
- [x] focused→全GUI/schema/型/build、独立仕様/品質reviewと限定修正を行う。

## Task 2: 既存lifecycle受入と同PRの日本語記録

- [x] 既存全版取下げ/公開終了のjourneyとpersistenceを最小拡張し、URL手入力なしの「文書履歴」入口→同ID選択→history read/旧原本hash/イベントを確認する。通常detailは404のまま、版・文書・ReadState等の元stateが不変であることを確認する。
- [x] 既存fixture/case/runnerを増やさず、timeout/retries/画像保存条件も変えない。純粋検査・runtime型/MCP compile・収集を確認する。
- [x] 日本語手順/実行状況/Activeを同PRへ含め、全組合せの独立reviewを行う。導入pinの収録範囲と、実GUI100件超・画像/macOS・本番Identity/TLS・対象PC・backup/restore・PG再起動等の未資格を保持する。
- [ ] 同headの通常必須CI・実行済みruntime gate・freshDB・cleanup/artifact0を確認する。stdoutを取得できない場合は値を捏造せず、既存の強制終了条件と公式step結果の対応を直接観測と区別する。新たな合格条件や緩和を加えない。

main mergeは親、実サーバー反映は所有者が手動で行う。停止された別プログラムや、拒否されたログ取得の別経路へ進まない。

2026-10-06 21:44 UTC: GUI1351/54・focused326/6・schema/型/build、既存受入sourceの純粋81/型/MCP compile/収集18+5が成功。独立230/5・schemaと日本語追補のreviewもGO。hosted資格の項目は未完了のまま保持する。

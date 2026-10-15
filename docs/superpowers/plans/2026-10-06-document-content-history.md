# コンテンツ版履歴を閲覧専用で開く小計画

## 目的と承認範囲

所有者の「内部処理があるものを通常GUIから操作できるようにする」指示と、機能単位のPRをmainへ統合する方針に従う。基点は資格済みmain `c2b68850aa022ac77ff180e5010c2197f949036d`。Frozen GUIのVersion/正式Revision分離・現在認可・capability方針と、既存 `purpose=history` 契約を使う追加UIである。

通常に読める文書の「版・改訂」内から、コンテンツ版の一覧、明示選択した版の詳細と原本を閲覧できるようにする。現在の公開・編集対象、Version lifecycle、ReadState、Document metadataを変更しない。原本開示には既存の認可・必須auditが適用される。通常readからhistoryへの暗黙fallback、公開終了文書用route、明示既読操作、backend/OAS/SDK変更は含めない。

未mergeの文書イベント履歴PR91のsourceに依存しない。別worktreeでmainから実装し、必要な通常main追従は後で行う。新基盤・大量fixture・画像/golden更新は追加せず、GUI・試験・日本語手順を同機能1PRへまとめる。

## 固定するUI/API契約

- 明示入口は「コンテンツ版の履歴を開く」、領域名は「コンテンツ版の履歴（閲覧専用）」。閉じる操作を設ける。既存の「版」「正式改訂」「変更履歴」と区別する。
- 既存list/detail/files/downloadへ必ず `purpose=history` を渡す。一覧は100件と返却opaque cursorを保持し、空/短いpageでもcursorがあれば継続する。versionIdで重複を除きserver順を保つ。offset cursorは完全snapshotを約束しない。
- 旧版選択は専用stateとし、既存URLのversionId、selectedVersion、公開/予約/編集対象へ代入しない。明示IDが無ければ別版へfallbackしない。返却された現行・WORKING・PUBLISHED・WITHDRAWNの状態をそのまま表示する。
- 選択詳細のtitle、Version metadata、日時、本人firstReadAtだけを表示し、現在Document metadataを過去の値と解釈しない。コンテンツ版番号と正式改訂番号/OCC revisionを混同しない。
- 原本取得は選択詳細のdownload capabilityがavailableで、同一対象のfiles取得に成功した場合だけ提示する。返却role=AUTHORITATIVEの全行をserver順で扱い、返却item/representation IDをそのまま使う。先頭1ファイル限定や未知roleの原本扱いはしない。
- historyのmutation capabilityは操作へ接続しない。ボタンの表示を最終認可の代わりにせず、各read/downloadの現在認可を維持する。
- 成功cacheは通常のversions/detail/filesと別keyにし、既存のdocument-versions/document-version/document-version-files prefixでの変更後失効に含める。閉じる/別tab/文書変更/route退出で旧成功データと選択・cursorを捨てる。未確定mutation・operation store・upload Blob・Organization providerは保持する。
- network/retryable 5xxの追加page失敗だけ既取得一覧と同cursor再試行を保つ。401/403/404・stale/validation等では古い一覧/詳細/原本を隠し、明示先頭再読取まで停止する。履歴だけの拒否から通常文書の拒否を推測しない。通常詳細の現在拒否は履歴readも失効させる。
- 取消済み旧要求の遅延成功/拒否を新しい対象へ注入せず、downloadはAbortSignalと対象固定を使い、選択変更/閉じる/失効後の遅いBlobを保存しない。read失敗を「原本なし」やダウンロード成功として表示しない。

## Task 1: 閲覧専用GUIと反例

- [x] 既存実route DOM/API harnessで、明示入口・選択隔離・返却state・100+1/cursor・全AUTHORITATIVE原本の正確な対象転送をREDにする。
- [x] mainの既存hook/表示部品/ApiFeedback/日時表現を再利用する小さい専用hook/componentと入口を追加する。PR91の未統合sourceを取り込まない。
- [x] 終端拒否・明示再読取・cache失効・通常詳細拒否・同cursor再試行・連打・中断/遅延・download保存抑止・通常操作/UNKNOWN保持をTDDで確認する。
- [x] focused→全GUI/schema/型/build、独立仕様/品質reviewと限定修正を行う。

## Task 2: 既存実受入と同PRの日本語記録

- [x] 既存regulation seedの初版/新版を使い、通常GUIから旧版を選んだdetail/files/download、原本hash、通常選択・ReadState・authoritative snapshot不変を既存journeyへ加える。新case/fixture/runnerを作らない。
- [x] 既存regulation persistenceへ同じ閲覧を加え、HTTP再起動後も明示history-purposeの旧版を取得する。既存18+5の構成・timeout/retriesと画像保存なしを保持する。
- [x] 純粋受入検査・runtime型・収集を確認し、日本語手順・実行状況・Activeを同PRへ含め、組合せ独立reviewを行う。
- [ ] 日本語Draftの同head既存hosted CI/実受入・cleanup/artifact0を確認する。実GUIコンテンツ版101件目・画像/macOS golden・本番Identity/TLS・対象PC導入・backup/restore・PGプロセス再起動等は未資格のまま記録する。

main mergeは親、実サーバー反映は所有者が手動で行う。停止された別プログラムの調査・変更や、拒否されたlog取得の迂回は行わない。

# Linux手動導入手順書の状態

## 2026-10-05 11:31 UTC — 通常GUIを入口とする手動導入手順

- 対象：Linux手動導入、Organization Browser PoC、文書GUI、本記録の4文書。サーバー反映は所有者の手動操作であり、この文書更新はdeployを行わない
- 資格状態：固定の模擬利用者2名・画像保存なしのUbuntu機能受入に合格した版（対象PCでの手順実行、本番認証、見た目全体の比較検証は対象外）。固定ソースは最終受入main `3d8deb253de19cb0954aa70a9a31cc5c4fc7540c` / tree `3f1ac6aa9e66d58bd5f01316e46334a48a64664f`
- PR69初回登録、70取下げ・公開終了、71属性編集、72予約取消、73 WORKING backendを保持し、PR74の既存複数原本編集・固定要求再送・Organization「編集作業」入口を対象とする。PR74 exact `ce56801f7ec73ed284a99838f07cfe0c92cf71f4` / tree `3f1ac6aa9e66d58bd5f01316e46334a48a64664f` の[通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37298371770)、[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37298371873)、[Sandbox](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37298371821)：required-checkを含む通常CI13/13・DSI・Sandboxが成功。Rust1599成功/9skip、指定実DB36成功、GUI404・runtime補助試験161成功。Document18件とHTTP再起動後5件、Agent9項目/provenance、Organizationの操作・往復・再起動・owned cleanup、公開artifact0を確認した。初回PUT・新版POST・続くPUTで、実成功応答のbody途中喪失から実headers/同一requestの失敗→UNKNOWN→同一要求の明示再送・結果一致・DB snapshot不変を確認。status/headersも全喪失する旧faultのGUI明示再送は未合格のままで、今回へ付け替えない。統合後main [push CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37301558995)：main自身のpush CIでrequired-checkを含む13/13 jobsが成功。Rust1599成功/9skip、指定実DB36成功。Document18件とHTTP再起動後5件、Agent9項目/provenance、Organizationの通常ナビ往復・操作・再起動・owned cleanup、公開artifact0を、PRとは別のmainログで確認した。exact head/clean、PostgreSQL18.6、固定合成2profileを照合した。作業版の固定再送資格は実成功応答のbody途中喪失に限定する
- Linux節6とOrganizationの共有文書準備を、ブラウザー所在PCの合成UTF-8ファイル→「文書」→「System Root」→単原本の下書き登録→authoring「版・改訂」→今すぐ公開・対象確認・ダイアログ確定→概要のDocument ID→サーバー側の既存 `runtime.env` / `seed-work` へ接続した。初回登録の結果不明は照会だけとし、再POSTしない
- 文書GUIには共通属性3項目と明示削除、既存複数原本の選択差替え、未変更原本・変換物・旧公開の保持、変更原本の旧変換物だけ新WORKINGから除外する条件を記す。原本追加・削除・並替・形式変更・自動再生成は追加しない。公開確定unknownで旧公開維持を断定しない
- 起動/停止/backup/restore、DB/storage/秘密情報保護、旧Search9のSTOP、停止中Search/Audit/Toolboxの境界と過去履歴を保持した。Organization用schedulerは起動せず、`KP_RUNTIME_MODE=poc` の既存起動例を流用しない
- 下書き作成時の静的照合では、参照source `e3f038ef0cfd49d564483b0d1339fd4a49f51a50` / tree `9b8a98fc47ac856d1b56e46b11304e499d9ee247` のOrganization CLI/config/identity/bootstrap、toolchain、Document/Work migrationは旧固定版 `6c514850850110a3c2f8b2b5664ec263510c5d47` から差分なし。最終受入mainの再照合結果：main 3d8deb253de19cb0954aa70a9a31cc5c4fc7540cでもOrganizationサーバーのCLI/config/identity/bootstrapを含むsource、固定toolchain/lock、Document/Work migrationとWork台帳実装は旧固定版6c514850から差分なし。Document repositoryのlib.rsはedit_manifest module宣言だけが増え、migration処理のbytesは不変。この参照sourceの記載は実受入合格の主張ではない
- 下書きの事前レビュー補正：公開結果不明は確認ダイアログの「同じ内容で再試行」を使い、版・公開方法・予約日時の変更、公開画面の開き直し、画面離脱・再読込・タブ終了を避ける。元の要求を失った場合は新規公開要求を送らず管理者に確認する。ダイアログを閉じるだけで必ず操作IDが失われるとは扱わない。また、明示的な最新確認に成功して新編集基準を採用した場合の未送信入力初期化と、文書名の控え・差替ファイルの再選択を説明する。失敗した読取りやUNKNOWNに対する強制リセットとはしない
- 静的検査結果：Bash例15個（Linux 12、Organization 3）の構文、相対リンク27件、4文書の差分空白検査、既存履歴・変更対象外コマンドの保持、placeholder対応表を確認。コマンド本体は未実行。コマンド本体・browser・DB・socket・実サーバー接続は実行していない
- 限定独立文書レビュー：確定したmain/CI値を入れた完成版4文書をread-onlyで最終確認しGO。未解決Critical/Important/Minorなし。文書公開後のdocs-only exact-head CIは別途確認し、製品sourceの既存資格や本手順全文の実行証拠と混同しない
- 残る限界：対象PCのdistribution/version・実運用設定は未確定。手順全文、常設backup/restore、PostgreSQLプロセス再起動は未検証。画像なしUbuntu PoCのみを資格対象とし、macOS goldenは未実行・未更新。影響候補Mock 2・3・4・7と、他3枚の画素不変も未証明
- 次の操作：最終文書レビュー後、この4文書のみの日本語Draft PRを公開して通常CIを確認する。製品コード・migration・workflowを変更せず、main統合は親担当が行う。対象PCへの導入は所有者が別途実行する

以下は過去の文書更新時点の記録であり、今回のソース・文書・対象機の資格へ付け替えない。

---

## 2026-10-05 02:18 UTC — 統合済みPoCへ手順を同期する候補

- 固定版をmain `6c514850850110a3c2f8b2b5664ec263510c5d47`（合格PR67 `a39c90c2` と同tree `880b1a57`）へ同期する。Document migration1〜11/Work別台帳1〜6、合成Agent・完了・保留再開・公開原本取得を現在sourceと照合した
- 既存の起動/停止/backup/restore/権限・秘密情報の扱いは保持する。実行例の変更は固定source SHAだけ。12個のLinux手順と4個の操作手順のBash構文、相対リンク、diff検査は成功。コマンド本体の実行は行っていない
- [PR67](https://github.com/AIrisu-072/knowledge-platform/pull/67)の全CIと実受入は成功。main push [CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37253316995)も13 jobs・Rust1566件/9skip・実DB/2名操作/再起動/復元/cleanup成功、公開artifact0。初回read失敗/encoding観測の原因未特定を残し、対象PCの実手順・backup/restore・PostgreSQL再起動は未検証
- 独立branch `docs/organization-integrated-manual-20261005`。手順2文書と本記録の3文書のみで、製品sourceやSearch作業を変更しない。既存履歴は以下に保持する
- 限定独立文書レビューGO、未解決Critical/Important/Minorなし。元CLI/settingsと整合し、旧履歴を保持する。今回のdocs-only headのCIは未実行で、実機の手順一式を試したとは扱わない
- 次のexact action: 日本語Draft候補を親へ返す。公開/main mergeと実サーバーへの導入は別の実行段階として扱う

---

## 2026年10月4日 UTC

- 対象：所有者がLinuxサーバーへ手動導入するための[日本語手順書](../../operations/linux-manual-installation.md)
- 基点：PR57 `d383baccddd5081687b500f064f6fce195a24816`。独立branch `docs/linux-manual-installation-20261004`
- 範囲：架空データ限定の固定2名Browser PoC、秘匿設定、新しい専用DB、明示migration/bootstrap/seed、起動、停止、更新、backup/restore、切戻しと本番未達チェックリスト
- 対象PCはCore Ultra 9 285K、Linux方針のみ確定。distribution/version、実接続先、容量、運用設定は未確定
- Agent次slice、Search、Audit、Tauriの完成や本番稼働を主張しない。進行中branchは変更しない
- 静的検証：既存CLI・config・runbookとの照合、Bash例12個の構文、相対リンクの存在、追加2文書だけの変更範囲、staged diff検査、既存repository policyがPASS。コマンド本体、新たなDB・listener・browser・credential・deployの実行なし
- 実機での手順一式、backup/restore、PostgreSQL再起動後の受入は未実施。基点のhosted CIはこの新しい手順書の実行証拠ではない
- 限定文書レビュー：統合担当が全文と現CLI/config/health/初期化を照合しGO。全writer停止、DB/storageの一組保存、元を残す別DBへの復元、本番未達の表示を確認した。補足の静的修正はPDFium取得失敗の伝播とPostgreSQLのTCP readiness指定のみで、構文と元harnessの方式を再確認済み
- 現在：文書のみの新Draft公開とexact-head CI確認の準備完了。基点や進行中の他branchを変更せず、公開後の実結果はPR本文に記録する。対象PCでの実手順は未検証のまま、OS決定時にOS固有の準備を追記する

## 統合調査の注意

2026年10月4日14時台UTCのGitHub照合ではmainは `d71753d46590bb4406a1c0b74894ab90a27a6c88`、main CI成功。PR36〜57のうちPR52/53は標準CI失敗。PR40の `0610b49327cd3c1c37e385281f087423c27b5638` は標準CI・DSI・Sandbox全成功だが、P7とSearch全体受入は別gateである。

DocumentとSearchはそれぞれDocument migrationのversion 9を追加している。適用済み台帳を確認せず番号・checksumを変更しない。GitとDB/storageのrollbackを分離する。既存のGitHub Actionsに自動production配備は見つからず、GitHub environment/deployment履歴はconnectorの読取対象外で確認できていない。実本番接続・配備は実施していない。

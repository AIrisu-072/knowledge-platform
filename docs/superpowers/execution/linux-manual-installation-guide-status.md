# Linux手動導入手順書の状態

## 2026-10-06 17:06 UTC — PR89統合mainへ固定版を同期

- 対象はLinux手動導入、Organization Browser PoC、文書GUI、本記録の4文書。比較結果の続き表示と同じ機能PR内で、導入pinを旧PR87公開製品head `cd6aafcc4e914050d8fc0e0f85483d82572e29da` / tree `dfba74428ef342d43369b094bd9e5117f3ee9fb4` からPR89統合main `e249fb8da91549115d1371c05959e3219dbfde1c` / tree `c8188d99b33b52ce36383c96d19e0d9f39fcb92c` へ更新する。文書自身のcommitをpinにしない
- 新pinはPR88文書移動、PR89正式改訂の続き表示・明示比較選択の保持を含む。今回の比較結果の続き表示は含まず、GUIの新節で後続source向けと明記する。旧PR87/PR81/PR74以前の受入数値・run URL・確認時刻は過去履歴へそのまま保持する
- main資格の最終確認：2026-10-06 17:06:10 UTC。[main自身のpush CI37497603490](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37497603490)はattempt1・全13jobs/13checks成功、failure/skip/未終端0、終端後公開artifact0。このmainのpush workflowは1runであり、PR89側の4workflow/18checksとは別の実行である。終端後のmain ref/tree/両parents（旧main ea406848・PR89 head075a1e79）も一致した
- [実runtime job112386056459](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37497603490/job/112386056459)の正式stdoutでactual head=e249fb8、gitDirty=false、acceptanceQualified=true、Document全22工程、journey18件＋HTTP再起動後persistence5件、Agent9項目/provenanceVerified=true、restartIdentityVerified=trueを確認。GUIも同mainログの47 suites/1161件PASSであり、PRのローカル1160件や後続featureの件数を流用しない
- [Rust実DB job112386055919](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37497603490/job/112386055919)もmain e249自身のcheckout。workspace1849成功/既存skip10、追加21成功/skip0、別追加7成功/既存skip1。指定実DB36/36とFolder回帰4/4を各1本の正式PASS行で照合し、missing/ambiguous各0。個別state値は同tree assertionとPASSの対応であり、全値の直接ログとは扱わない
- journeyのmetadata個別PASSは直接公開されている。一方persistenceの有限tests配列はtruncated=trueでmetadata個別行を省いており、再起動後の文書移動replay・正式改訂read/比較・snapshot/readState保持は5/5 phase PASSと同treeの選択sourceを対応させた推論である。metadata個別PASS行を直接観測したとは記録しない
- Organizationは同runtime jobでbuild/database/transaction/initialize/journey/restart/persistence/shutdownの8phaseと `cleanup: owned-container-removed` を直接stdoutで確認。既存journey2＋persistence2の個別操作・HTTP2 process再起動後の内容は同tree sourceとphase PASSの対応推論。Document cleanupも最終qualified/passと同sourceのcleanup失敗条件からの推論であり、個別PID/CIDの生receiptはない。HTTP再起動をPostgreSQLプロセス再起動へ読み替えない
- 環境は既存PostgreSQL18.6・固定toolchain・合成profileによる画像なしPoC。WORKING固定再送の実通信資格は成功応答body途中喪失だけで、status/headers全喪失は未資格のままである
- 文書移動の同権限Shared→Sandbox1回、元要求replayと正式改訂/Version/原本hash/本人・Agent readState不変、正式改訂2件の通常読取・1.1→1.0の明示比較・一覧先頭再読取後の同pair新POST・HTTP再起動後の読取は同tree sourceと正式PASSを対応させた推論であり、個々のHTTP応答・assertion値の直接ログではない。有限stdoutで再起動後metadataの個別行は省略されている
- 固定Git objectの限定照合では旧cd6aafcc→e249のOrganization全tree（CLI/env/identity/bootstrap）、Document 0001〜0011/Work 0001〜0006のmigrationと台帳lib、Cargo manifest/lock/deny、mise/rust設定、Node/pnpm関連manifest/lock、生成SDK/API schema/PDFium取得scriptの指定17objectsは全て同一。差分はGUI/試験/docs/有限診断の35paths。新依存・migration/reset・env・本番Identity手順は追加せず、対象実DBの更新可能性や全面互換を主張しない
- コマンドは既存15shellblocksを保持し、変更はLinuxの `KP_SOURCE_SHA` 1行だけ。起動/停止/backup/restore/更新切戻し、Work9項目、秘密情報保護、合成2profile・localhost限定、scheduler不起動、旧Search9/不明台帳/checksumの停止条件を保持する。停止中Search/Audit/Toolboxの内容調査や追加作業はしない
- 所有者が今後行う文書移動・正式改訂の読取/比較/HTTP再起動の手動確認を未チェックで追加する。100件以下の対象機を101件目以降の実GUI合格とは記録しない。実サーバーへの反映は所有者の手動操作である
- 新しい比較GUI手順は固定3ラベル、本文差分と未比較範囲の両方の続き、メタデータ1回表示、取得済み未比較件数、固定pairとURL、一時失敗の同cursor再試行、stale/入力不一致と認可拒否を分けた明示再読取を説明する。新featureの実runtime資格は同機能PRの既存hostedで別途確認する
- 静的検査：Bash構文15個（Linux12、Organization3）、相対リンク49件、限定4path差分、旧受入履歴・Work9項目・停止/復旧/更新本文の保持、コマンド差分がpin1行だけであることを確認。コマンド本体、実サーバー、DB/socket/browser/Cargo、package install、画像、deploymentは今回の文書更新で実行していない
- 残る限界：正式改訂実GUI100件超・比較結果実GUI50件超、画像/macOS golden/全visual、WORKING全status/headers喪失、実ACL変化・GUI移動通信断・実GUI同親no-op、PR87フォルダー改名での実文書folderName更新、対象PCの手順全文、本番Identity/TLS、backup/restore、PostgreSQLプロセス再起動は未資格。旧PR82 persistence失敗とPR83 Organization503、PR89の過去失敗・有限診断訂正・Home focus残件を後続成功だけで解消済みとしない
- 次のexact action：この4docsと機能sourceを独立確認し、比較続き表示と同じPRへ収録して最終CI/実受入を確認する。結果だけの再commit/別PRを作らない。main mergeは親担当、実サーバーへは所有者が手動反映する

以下は当時のpin・資格・文書更新方針の履歴であり、現在mainや後続feature、対象PCへ資格や次操作を付け替えない。

---

## 2026-10-06 09:11 UTC — PR87受入済み公開製品headへ固定版を同期

- 対象はLinux手動導入、Organization Browser PoC、文書GUI、本記録の4文書。同じPR87内のpin更新として、製品と実受入を含む受入済み公開製品head `cd6aafcc4e914050d8fc0e0f85483d82572e29da` / tree `dfba74428ef342d43369b094bd9e5117f3ee9fb4` を固定する。基点mainは `b9f447faa294f1898ef2b1d375b055c4b9e96cd8`。公開製品headの資格とmain統合結果は別に確認し、文書更新commit自身のSHAを本文へ書かない
- PR81までの旧固定版0801の範囲を保持し、PR82未読条件、PR84作成日時条件、PR87移動GUI・限定mapper・既存受入を含める。移動操作本文、旧0801/PR81・3d8/PR74等のSHA・CI数値・確認時刻は保持し、旧headの資格を今回へ転用しない
- 製品資格の最終確認：2026-10-06 09:11:48 UTC。固定合成2profile・画像なしUbuntu機能受入の資格であり、main統合結果や対象PC導入の資格とは分ける
- [通常CI37438675289](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675289)は13/13 jobs成功。[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675264)・[Sandbox Preflight](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675307)も成功し、[Organization D2](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675413)は既存適用条件によりskip。全18checksは15成功/既存skip3/failure0、全4runはattempt1で終端。DSIのmacOS qualificationとOrganization D2の2jobsの既存skipを、画像・golden資格へ転用しない
- [実runtime job](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675289/job/112186722085)は公開製品head自身のcheckout、gitDirty=false、acceptanceQualified=true、全22stages passedを示す。通常RustのPR検査用merge `6ea0486f3057b83457cd2711849007e723ddde1d` は同treeで、parentsは基点mainと公開製品head。ローカルGUI952件/42 suites・schema/型/build成功と独立SOURCEレビューGOはローカル資格として区別する
- [Rust実DB job](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675289/job/112186722380)で指定36件を今回の正式PASS行と36/36照合し、新規同名衝突のHTTP/Repository2反例と既存拡張no-op/replay/stale caseもPASS。HTTP409/REVISION_CONFLICT・台帳0、rollbackとFolder/Document/access・台帳/イベント/監査不変は同treeの該当assertionと正式case PASSの対応から確認し、個別state値の直接ログとは扱わない。workspace1849成功/既存skip10、追加21成功、別追加7成功/既存skip1、全failure0
- Organizationは同[実runtime job](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675289/job/112186722085)でbuild/database/transaction/initialize/journey/restart/persistence/shutdownの8phaseがpassed。既存journey2＋persistence2の移動・fresh GET/DOM・201件・通常ナビ往復の詳細は、固定sourceの同2+2 caseとphase成功を対応させた推論である。個別case名・個々のassertion値は公開stdoutに出ていない
- Document18件＋HTTP再起動後5件は全PASS・fail/skip0。属性・未読・日時の実GET/往復と本人/Agent readState不変を含む既存受入を保持し、Agent9項目・provenanceVerified=trueを同[実runtime job](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675289/job/112186722085)で確認した。WORKING固定再送の実通信資格は成功応答のbody途中喪失に限る
- Document側HTTP再起動ではrestartIdentityVerified=true。Organizationの2HTTP process再起動後の現在親・同ID/同名/revision 2、元create/rename/move receiptのsales固定replay・office403と前後read不変は同sourceのpersistence caseとphase成功からの推論。HTTP再起動をPostgreSQL process再起動へ読み替えない
- Organizationのcleanupは正式logのowned-container-removedで確認。Document cleanupは最終passed summaryと同sourceのfail-closed cleanup経路からの推論であり、個別PID/CIDの生receiptはない。全4runの終端後公開artifact0を確認した
- test-first head db9031dbの実HTTP500/期待409 REDは履歴として保持する。当時fail-fastで未実行だったRepository衝突・追加no-op/replayは当時のREDやPASSと扱わず、今回Pの正式case PASSで初めてGREENを確認した
- この文書更新の同PR最終CIは公開後に確認し、実結果を同PR本文へ記録する。本記録時点で文書更新後のCI資格は未取得であり、pin先製品の実資格と分ける。結果記録だけの再commitや独立docs-only PRは作らない
- 固定Git objectの限定照合では旧0801から同treeのsource45c6d05まで、既存CLI/env、Document0001〜0011とWork0001〜0006のSQL/台帳、Node/pnpm/生成SDK/schema/PDFium仕様は不変。既存15shellblocksは `KP_SOURCE_SHA` の1行だけをPへ変更する。共通Cargo.toml/Cargo.lock/deny.tomlは基点main側で更新済みのbytesを保持し、新lockで既存 `--locked` debug buildとGUI buildを使う。依存追加・`cargo update`・新migration/reset・本番Identity手順は増やさず、全面互換や対象実DBの更新可能を主張しない
- 所有者の未実施手動確認に、合成子の移動/現在親/HTTP再起動、未読条件の往復/解除、作成日時の範囲/往復/解除を追加した。対象PCで未実施のチェックをCI成功で埋めない。今回の文書更新でコマンド本体、実サーバー、DB、Cargo、browser、画像、installは実行していない
- 残る限界：macOS golden/画像/全visual、WORKING全status・headers喪失、実ACL変化、GUI移動通信断・実GUI同親no-op、実文書folderName更新、対象PCの手順全文、本番Identity/TLS、backup/restore、PostgreSQL process再起動は未資格。今回の同Root継承・文書なしfixtureとHTTP2 process再起動の範囲を広げない。旧PR82 persistence失敗とPR83 Organization HTTP503の原因未特定履歴を、後続head成功だけで修正済みとしない
- 次のexact action：この4docsを独立確認し、同PR87へ更新を収録して最終CIを確認する。実結果は同PR本文へ記録する。以下の過去記録にある別PR方針は今回へ継承せず、実サーバーへは所有者が手動で反映する

以下は当時の固定版・文書更新方針の履歴であり、今回の公開製品head・同PR更新・対象PCへ資格や次操作を付け替えない。

---

## 2026-10-06 03:30 UTC — PR81までの受入済みmainへ固定版を同期

- 対象はLinux手動導入、Organization Browser PoC、文書GUI、本記録の4文書のみ。固定ソースをmain `0801c9864bdb7faf5fcbe7ee1062367335ee7bfb` / tree `794407fc62e2418bd6ecbdc2de4541cd3e207b02`へ同期する。PR76 Root直下作成、PR78続き表示、PR79選択親への子作成、PR80改名、PR81属性3項目の絞り込みを含み、未公開の後続GUIは含めない
- PR81公開head `426db3a5a43083a1b4bd76b656ea58322019eed3` / 同treeの[通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37405694808)全13jobs、[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37405694768)、[Sandbox](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37405694748)は成功。全18checksは15成功・既存条件skip3・failure0、Organization専用workflowの既存skipを含む全4runの終端後公開artifact0
- 別のmain自身の新[push CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37407454204)も全13jobs/checks成功・failure0・skip0、終端後公開artifact0（03:27 UTC確認）。今回mainの実ログでGUI788件/39 suites、Document18件＋HTTP再起動後5件、短い合成3属性の実GET一致/不一致/解除・詳細往復、Agent9項目/provenance、Organization buildを含む全8stages（固定source/configの2+2に対応）、owned cleanupを確認した。Rust1831成功/9skip、別feature suite21成功と7成功/1skip、今回指定実DB36のPASS名36/36を照合した
- Organizationのbuildは手動手順と同じ既存debug build経路。exact main checkout、Document summaryのcleanと同tree runnerの必須clean gate通過を併せて確認し、Organization固有のhead/dirtyが公開logへ単独出力されたとは記録しない。hosted build成功は対象PCでのビルド・手順全文の実行証拠ではない
- 旧固定3d8deb2/PR74の数値・run URL・時刻、各フォルダー機能の初回統合SHA/CIを履歴として保持した。Work9項目、単原本登録→公開→Document ID→seed-work、scheduler不起動、起動/停止/backup/restore/更新切戻し、旧Search9・不明台帳/checksumの停止条件と本番未達を保持する
- 所有者が今後行う手動確認として、Root子作成、選択親への子作成、続きを表示、同ID改名/HTTP再起動、3属性一致/不一致/解除/詳細往復の未実施チェック5項目だけを追加した。対象機が200件以下なら201件表示の合格とは記録しない。新テンプレート・検証基盤・コード・fixtureを追加しない
- 旧3d8→新0801のGit object限定照合：Organization-server全tree（CLI/config/identity/bootstrapを含む）、Document migration0001〜0011/lib.rs、Work migration0001〜0006/lib.rs、Node/pnpm関連manifestとpnpm lock、GUI package、PDFium取得scriptは同bytes。scalar pins Rust1.98.1/Node24.21.0/pnpm12.4.1とPDFium151.0.7881.0は不変。一方、共通Cargo.toml/Cargo.lock/deny.toml/mise.toml/CI、共有outbox/architecture policy等は変更pathあり。全lock不変・全面互換・対象DBへの更新可能を主張しない。Search関連は変更pathの把握だけで、内容・依存・migration・停止作業の追加調査はしない
- 静的検査結果：Bash構文15個（Linux12、Organization3）、相対リンク33件、固定SHA/treeと変更4path、旧受入履歴・Work9項目・停止/復旧/更新のbytes保持を確認。実行例の差分はKP_SOURCE_SHAの1行だけ。コマンド本体は未実行。実サーバー/DB/socket/browser/Cargo/package install/画像作業、GitHub write/merge/manual workflowは行わない
- 残る限界：対象PCのdistribution/version・手順全文、production Identity/TLS、backup/restore、PostgreSQLプロセス再起動、macOS golden/full visual/画素不変、WORKING全status/headers喪失、Folder実no-op/実文書folderName更新/実通信断、新原本の追加/削除/並替/初回複数登録、日時/未読条件、移動/ACL/既読記録、実LLM/外部MCPは未資格または未対応。既存の旧公開維持・atomic publish・body途中喪失の限定資格を拡張しない
- 次のexact action：この4文書だけを独立read-onlyレビューへ渡す。親担当が日本語の独立小Draft PRを公開し、docs-only exact-head CIを別途確認する。main mergeはrootが直列化し、対象PCへの反映は所有者が手動で行う。この時点では今回文書の独立レビュー・公開後CIは未確認

以下の2026-10-05以前の記録は当時の資格であり、今回の新headや対象機へ付け替えない。

---

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

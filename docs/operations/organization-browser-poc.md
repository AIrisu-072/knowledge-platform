# Organization Browser PoC の起動と確認

## 対象ソースと確認状況

固定ソースは[PR93](https://github.com/AIrisu-072/knowledge-platform/pull/93)統合main `41b584ddea6c3c9ec90343f3ba98cfdac560bd24` / tree `591eb64a2d54912c2faf925b16ed70bb97265deb` で、[Linux手動導入](linux-manual-installation.md)と共通にする。固定合成2profile・画像なしUbuntu機能受入に合格した版であり、main自身の[push CI37553290752](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37553290752)をPR公開headとは別に確認した。対象PCでの手順実行や本番認証の資格ではない。固定SHAと受入記録が未確定の版は実行しない。

採用sourceは旧pin933dまでの文書GUI/Folder操作/絞り込み/文書移動/正式改訂・比較の続き表示・通常詳細のコンテンツ版/イベント履歴と旧原本を保持し、PR93の履歴一覧入口から公開終了/全版取下げ後の旧版・原本・イベントを読む操作と、共有履歴表示のJST指定修復を含む。この入口とJST修復は旧固定版933dに未収録。現在開発中の公開前WORKINGと現行公開版の内容比較GUIは新pin41bにも未収録で、追加される操作説明は後続source向けである。exact run/job URL・直接観測と対応推論の区別は[Linux手動導入の対象資格](linux-manual-installation.md#この手順でできること)を正本とする。

2026-10-07 01:35 UTCに証拠を整理し、公式GETのmain全13jobs成功・completed/successと公開artifact0を確認した。[runtime job112573599189](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37553290752/job/112573599189)のcheckout/Document/Organization/summary各step成功は直接観測した。runtime stdoutは元tool初回のTransport closedで未取得のため、head/clean/qualified・GUI/browser件数・provenance/再起動/cleanup receiptの印字値は未読のままである。固定mainのsourceが実build・全必須工程・Agent provenance・同じowned DB/storageでのHTTP再起動・owned cleanupを強制し、失敗時に非zeroとなることと今回成功stepを対応させ、既存必須gateを合格と評価した。PR/旧pinの値やローカルGUI1353/54を今回hosted値へ移さない。旧PR93 head202dcの共有失敗summaryも転用しない。

Document22工程/選択18＋5、Agent9 groups、Organization8工程/選択2＋2と、履歴一覧の公開終了/取下げ後の旧Version1単一原本・少数events、HTTP再起動後の同じ導線および既存操作の個別assertionは固定source構成と今回成功実行からの対応推論である。summaryはbrowser件数/skipped=0を直接検査しないため、no-skipの対応は固定選択source/configにskip/only/expected-failure経路がない範囲に限る。Organizationのowned-container-removedも今回は直接stdoutではない。HTTP再起動をPostgreSQLプロセス再起動へ読み替えない。[新Rust実DB job112573599264](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37553290752/job/112573599264)のmain41b checkout、1849成功/10skip＋21成功/0skip＋7成功/1skip、DB36/Folder4の各1本PASSは正式ログで直接確認した。

旧933d→41bの新しいGit show/object bytes照合で、既存CLI/env/identity/bootstrap、Document/Workのmigration/台帳、Cargo manifest/lock/deny、Rust/Node/pnpm設定/lock、生成SDK/schema、PDFium scriptの17objectsは不変。sourceの業務schema不変を対象既存DBの更新安全性の実証とはしない。実行例は保持し、Linux手順のKP_SOURCE_SHAだけを更新する。

正式改訂100件超・比較結果50件超・イベント履歴101件目・コンテンツ版101件目/実複数旧原本の実GUIは未資格。DOMや既存HTTPページ試験と区別する。WORKING固定再送の実通信資格は成功応答body途中喪失だけで、status/headers全喪失は未資格のままである。

資格対象は画像なしUbuntu実操作PoCであり、対象PCでの手順全文・backup/restore・PostgreSQLプロセス再起動の確認は未実施。macOS golden比較は未実行・未更新で、影響候補Mock 2・3・4・7の4枚と、他3枚の画素不変も未証明。全visual資格や本番Identityの資格は主張しない。PR87の同Root継承・文書なしのフォルダー移動fixtureとPR88の同権限の文書移動fixtureでは実ACL変化・GUI移動通信断・実no-opの資格を追加しない。旧PR82 persistence失敗とPR83 Organization HTTP503、PR89の過去失敗・Home focus残件、旧PR91 head81be7976のstdout未取得、PR93の過去失敗とstdout未取得を後続成功だけで解消済みとしない。

### 過去の受入記録

#### 2026-10-06 21:24 UTC PR91統合mainの固定版

以下の「固定版」「今回」「未収録」は当時のpinと履歴一覧GUI開発時点を指す。

固定ソースは[PR91](https://github.com/AIrisu-072/knowledge-platform/pull/91)統合main `933d3b0f894e610496022defae8e494b16de39ea` / tree `8c6789bc3ae0332894ab1dea8f1b84686444a611` で、[Linux手動導入](linux-manual-installation.md)と共通にする。固定合成2profile・画像なしUbuntu機能受入に合格した版であり、main自身の[push CI37530751555](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37530751555)をPR公開headとは別に確認した。対象PCでの手順実行や本番認証の資格ではない。固定SHAと受入記録が未確定の版は実行しない。

採用sourceは旧pin e249までの文書GUI/Folder操作/絞り込み/文書移動/正式改訂を保持し、PR90比較結果の続き表示、PR92通常詳細からの閲覧専用コンテンツ版履歴/旧原本、PR91イベント履歴の続き表示と両履歴の共存を含む。これら3機能は旧固定版e249に未収録。現在開発中の履歴一覧から旧版・原本・イベントを開く通常入口は新pin933dにも未収録で、追加される操作説明は後続source向けである。exact run/job URL・確認時刻・直接観測と対応推論の区別は[Linux手動導入の対象資格](linux-manual-installation.md#この手順でできること)を正本とする。

2026-10-06 21:24:51 UTC確認で全13jobs/13checks成功・唯一のmain runの公開artifact0。[runtime job112499205642](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37530751555/job/112499205642)のcheckout/Document/Organization/summary各step成功は直接観測した。runtime stdoutは元tool初回のTransport closedで未取得のため、head/clean/qualified・GUI/browser件数・provenance/再起動/cleanup receiptの印字値は未読のままである。固定sourceが実build・全必須工程・Agent provenance・同じowned DB/storageでのHTTP再起動・owned cleanupを強制し、失敗時に非zeroとなることと今回成功stepを対応させ、既存必須gateを合格と評価した。PR/旧pinの出力値やローカルGUI1312/53を今回hosted値へ移さない。

Document22工程/選択18＋5、Agent9 groups、Organization8工程/選択2＋2と、両履歴・比較・従来の文書移動/正式改訂の個別assertionは固定source構成と今回成功実行からの対応推論である。summaryはbrowser件数/skipped=0を直接検査しないため、no-skipの対応は固定選択source/configにskip/only/expected-failure経路がない範囲に限る。Organizationのowned-container-removedも今回は直接stdoutではない。HTTP再起動をPostgreSQLプロセス再起動へ読み替えない。[新Rust実DB job112499205731](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37530751555/job/112499205731)のmain933d checkout、1849成功/10skip＋21成功/0skip＋7成功/1skip、DB36/Folder4の各1本PASSは正式ログで直接確認した。

旧e249→933dの限定Git object bytes照合で、既存CLI/env/identity/bootstrap、Document/Workのmigration/台帳、Cargo manifest/lock/deny、Rust/Node/pnpm設定/lock、生成SDK/schema、PDFium scriptの17objectsは不変。sourceの業務schema不変を対象既存DBの更新安全性の実証とはしない。実行例は保持し、Linux手順のKP_SOURCE_SHAだけを更新する。

正式改訂100件超・比較結果50件超・イベント履歴101件目・コンテンツ版101件目/実複数旧原本の実GUIは未資格。DOMや既存HTTPページ試験と区別する。WORKING固定再送の実通信資格は成功応答body途中喪失だけで、status/headers全喪失は未資格のままである。

資格対象は画像なしUbuntu実操作PoCであり、対象PCでの手順全文・backup/restore・PostgreSQLプロセス再起動の確認は未実施。macOS golden比較は未実行・未更新で、影響候補Mock 2・3・4・7の4枚と、他3枚の画素不変も未証明。全visual資格や本番Identityの資格は主張しない。PR87の同Root継承・文書なしのフォルダー移動fixtureとPR88の同権限の文書移動fixtureでは実ACL変化・GUI移動通信断・実no-opの資格を追加しない。旧PR82 persistence失敗とPR83 Organization HTTP503、PR89の過去失敗・Home focus残件、旧PR91 head81be7976のstdout未取得を後続成功だけで解消済みとしない。



#### 2026-10-06 17:06 UTC PR89統合mainの固定版

以下の「固定版」「今回」「未収録」は当時のpinと比較GUI開発時点を指す。

固定ソースは[PR89](https://github.com/AIrisu-072/knowledge-platform/pull/89)統合main `e249fb8da91549115d1371c05959e3219dbfde1c` / tree `c8188d99b33b52ce36383c96d19e0d9f39fcb92c` で、[Linux手動導入](linux-manual-installation.md)と共通にする。固定合成2profile・画像なしUbuntu機能受入に合格した版であり、main自身の[push CI37497603490](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37497603490)をPR公開headとは別に確認した。対象PCでの手順実行や本番認証の資格ではない。固定SHAと受入記録が未確定の版は実行しない。

採用sourceはPR69〜PR74の文書GUI、PR76 Root直下作成、PR78フォルダーの続き表示、PR79選択親への子作成、PR80改名、PR81属性3項目、PR82未読条件、PR84作成日時条件、PR87フォルダー移動を保持し、PR88文書移動とPR89正式改訂の続き表示・明示比較選択の保持を含む。旧固定版 `cd6aafcc` にはPR88/89は含まれない。資格の実測値・exact run/job URL・確認時刻は[Linux手動導入の対象資格](linux-manual-installation.md#この手順でできること)を正本とする。

mainのDocument実受入、Agent、Organizationのbuild/database/transaction/initialize/journey/restart/persistence/shutdown、HTTP再起動・cleanupを確認した。文書移動と正式改訂2件の読取・明示比較・再読取の個別assertionは同tree sourceと正式PASSの対応推論であり、再起動後metadataの個別行は有限stdoutで省略されている。Document cleanupも最終qualifiedと同sourceの失敗条件からの推論、Organizationのowned-container-removedは直接stdoutである。HTTP再起動をPostgreSQLプロセス再起動へ読み替えない。

正式改訂100件超の実GUIは未資格。今回の比較結果の続き表示は固定版e249に未収録で、[新しい操作説明](document-gui-v0.md#比較結果の続きを表示する)は後続source向けである。比較結果50件超の実GUIも未資格。DOMや既存HTTPページ試験と区別する。WORKING固定再送の実通信資格は成功応答body途中喪失だけで、status/headers全喪失は未資格のままである。

資格対象は画像なしUbuntu実操作PoCであり、対象PCでの手順全文・backup/restore・PostgreSQLプロセス再起動の確認は未実施。macOS golden比較は未実行・未更新で、影響候補Mock 2・3・4・7の4枚と、他3枚の画素不変も未証明。全visual資格や本番Identityの資格は主張しない。PR87の同Root継承・文書なしのフォルダー移動fixtureとPR88の同権限の文書移動fixtureでは実ACL変化・GUI移動通信断・実no-opの資格を追加しない。旧PR82 persistence失敗とPR83 Organization HTTP503の原因未特定履歴を保持する。

#### 2026-10-06 09:11 UTC PR87の固定版

固定ソースは[PR87](https://github.com/AIrisu-072/knowledge-platform/pull/87)の受入済み公開製品head `cd6aafcc4e914050d8fc0e0f85483d82572e29da` / tree `dfba74428ef342d43369b094bd9e5117f3ee9fb4` で、[Linux手動導入](linux-manual-installation.md)と共通にする。固定合成2profile・画像なしUbuntu機能受入に合格した版であり、基点main `b9f447faa294f1898ef2b1d375b055c4b9e96cd8` からの製品資格とmain統合結果は別に確認する。対象PCでの手順実行や本番認証の資格ではない。固定SHAと受入記録が未確定の版は実行しない。

採用sourceはPR69〜PR74の文書GUI、PR76 Root直下作成、PR78続き表示、PR79選択親への子作成、PR80改名、PR81属性3項目の絞り込みを保持し、PR82未読条件、PR84作成日時条件、PR87移動・同名衝突mapper・既存移動受入を含む。旧固定版0801にはこの3機能は含まれない。資格の実測値・exact run/job URL・確認時刻は[Linux手動導入の対象資格](linux-manual-installation.md#この手順でできること)と共通にする。

- 製品資格の最終確認：2026-10-06 09:11:48 UTC。固定合成2profile・画像なしUbuntu機能受入の資格であり、main統合結果や対象PC導入の資格とは分ける
- [通常CI37438675289](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675289)は13/13 jobs成功。[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675264)・[Sandbox Preflight](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675307)も成功し、[Organization D2](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675413)は既存適用条件によりskip。全18checksは15成功/既存skip3/failure0、全4runはattempt1で終端。DSIのmacOS qualificationとOrganization D2の2jobsの既存skipを、画像・golden資格へ転用しない
- [実runtime job](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675289/job/112186722085)は公開製品head自身のcheckout、gitDirty=false、acceptanceQualified=true、全22stages passedを示す。通常RustのPR検査用merge `6ea0486f3057b83457cd2711849007e723ddde1d` は同treeで、parentsは基点mainと公開製品head。ローカルGUI952件/42 suites・schema/型/build成功と独立SOURCEレビューGOはローカル資格として区別する
- [Rust実DB job](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675289/job/112186722380)で指定36件を今回の正式PASS行と36/36照合し、新規同名衝突のHTTP/Repository2反例と既存拡張no-op/replay/stale caseもPASS。HTTP409/REVISION_CONFLICT・台帳0、rollbackとFolder/Document/access・台帳/イベント/監査不変は同treeの該当assertionと正式case PASSの対応から確認し、個別state値の直接ログとは扱わない。workspace1849成功/既存skip10、追加21成功、別追加7成功/既存skip1、全failure0
- Organizationは同[実runtime job](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675289/job/112186722085)でbuild/database/transaction/initialize/journey/restart/persistence/shutdownの8phaseがpassed。既存journey2＋persistence2の移動・fresh GET/DOM・201件・通常ナビ往復の詳細は、固定sourceの同2+2 caseとphase成功を対応させた推論である。個別case名・個々のassertion値は公開stdoutに出ていない
- Document18件＋HTTP再起動後5件は全PASS・fail/skip0。属性・未読・日時の実GET/往復と本人/Agent readState不変を含む既存受入を保持し、Agent9項目・provenanceVerified=trueを同[実runtime job](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675289/job/112186722085)で確認した。WORKING固定再送の実通信資格は成功応答のbody途中喪失に限る
- Document側HTTP再起動ではrestartIdentityVerified=true。Organizationの2HTTP process再起動後の現在親・同ID/同名/revision 2、元create/rename/move receiptのsales固定replay・office403と前後read不変は同sourceのpersistence caseとphase成功からの推論。HTTP再起動をPostgreSQL process再起動へ読み替えない
- Organizationのcleanupは正式logのowned-container-removedで確認。Document cleanupは最終passed summaryと同sourceのfail-closed cleanup経路からの推論であり、個別PID/CIDの生receiptはない。全4runの終端後公開artifact0を確認した

#### 2026-10-06 03:27 UTC PR81までの固定版

以下は旧固定版0801の記録であり、現在pinの公開製品headへ資格を付け替えない。

導入対象の資格：固定の模擬利用者2名・画像保存なしのUbuntu機能受入に合格した版（対象PCでの手順実行、本番認証、見た目全体の比較検証は対象外）。最終受入main `0801c9864bdb7faf5fcbe7ee1062367335ee7bfb` / tree `794407fc62e2418bd6ecbdc2de4541cd3e207b02` を[Linux手動導入](linux-manual-installation.md)と共通の固定ソースにする。固定SHAと受入記録が未確定の版は実行しない。

GUI統合の確認：PR69初回登録、PR70取下げ・公開終了、PR71属性編集、PR72予約取消、PR73 WORKING backend、PR74複数原本編集・固定要求再送・「編集作業」入口を保持し、PR76 Root直下作成、PR78フォルダーの続き表示、PR79選択親への子作成、PR80改名、[PR81](https://github.com/AIrisu-072/knowledge-platform/pull/81)属性3項目の絞り込みを含む。PR81公開head `426db3a5a43083a1b4bd76b656ea58322019eed3` / tree `794407fc62e2418bd6ecbdc2de4541cd3e207b02` の[通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37405694808)13/13 jobs、[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37405694768)、[Sandbox](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37405694748)が成功。全18checksは15成功・既存条件skip3・failure0で、Organization専用workflowの既存条件skipを含む全4runの終端後公開artifact0を確認した。

統合後main自身の[push CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37407454204)は、2026-10-06 03:27 UTC確認で全13jobs/checks成功・failure0・skip0、終端後公開artifact0。今回mainの別実行でGUI788件/39 suites、Document18件＋HTTP再起動後5件、属性3条件の実GET一致/不一致/解除・詳細往復、Agent9項目/provenance、Organizationのbuildを含む全8stages（固定source/configの2+2に対応）、owned cleanupを確認した。Rust1831成功/9skip、別feature suite21成功および7成功/1skip、指定実DB36件の今回PASS名36/36を照合。exact main checkoutとDocument summaryのclean、同treeのOrganization runnerの必須clean gate通過を確認したが、Organization固有のhead/dirtyが公開logへ単独出力されたとは扱わない。環境はPostgreSQL18.6・固定合成2profileで、作業版の固定再送資格は実成功応答のbody途中喪失に限定し、全status/headers喪失は未資格のままとする。

#### PR74以前の固定版

当時の固定ソースはmain `3d8deb253de19cb0954aa70a9a31cc5c4fc7540c` / tree `3f1ac6aa9e66d58bd5f01316e46334a48a64664f`。次の記録は2026-10-05 11:31 UTC時点のPR74までの資格であり、後続GUIや現在pinへ付け替えない。

Document GUIの追加はPR69初回登録、PR70取下げ・公開終了、PR71属性編集、PR72予約取消、PR73 WORKING backendと、[PR74](https://github.com/AIrisu-072/knowledge-platform/pull/74)の既存複数原本編集・固定要求再送・Organizationの「編集作業」入口を対象とする。PR74 exact `ce56801f7ec73ed284a99838f07cfe0c92cf71f4` / tree `3f1ac6aa9e66d58bd5f01316e46334a48a64664f` の[通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37298371770)、[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37298371873)、[Sandbox](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37298371821)の確認結果：required-checkを含む通常CI13/13・DSI・Sandboxが成功。Rust1599成功/9skip、指定実DB36成功、GUI404・runtime補助試験161成功。Document18件とHTTP再起動後5件、Agent9項目/provenance、Organizationの操作・往復・再起動・owned cleanup、公開artifact0を確認した。初回PUT・新版POST・続くPUTで、実成功応答のbody途中喪失から実headers/同一requestの失敗→UNKNOWN→同一要求の明示再送・結果一致・DB snapshot不変を確認。status/headersも全喪失する旧faultのGUI明示再送は未合格のままで、今回へ付け替えない。統合後mainの[push CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37301558995)の確認結果：main自身のpush CIでrequired-checkを含む13/13 jobsが成功。Rust1599成功/9skip、指定実DB36成功。Document18件とHTTP再起動後5件、Agent9項目/provenance、Organizationの通常ナビ往復・操作・再起動・owned cleanup、公開artifact0を、PRとは別のmainログで確認した。exact head/clean、PostgreSQL18.6、固定合成2profileを照合した。作業版の固定再送資格は実成功応答のbody途中喪失に限定する（2026-10-05 11:31 UTC）。

2026-10-05、[PR67](https://github.com/AIrisu-072/knowledge-platform/pull/67) `a39c90c2` / tree `880b1a57abc6890ed47df5e7bc16a4694d4546cc` で、合成Agent・完了・保留/再開・公開原本Downloadを含む同2名操作、実DB/transaction、両HTTP server再起動/復元/cleanupと[全通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37251574840)が成功した。統合後main `6c514850` は同一tree。この版は以前の[Linux手動導入](linux-manual-installation.md)の固定ソースであり、後続GUIや最終統合版の資格とは区別する。初回read失敗と原本bytes観測失敗の原因未特定という記録は残し、実サーバーや本番Identityの資格とは区別する。

以下は最初の最小経路の受入履歴である。

2026-10-04、[PR54のsource44e1b412](https://github.com/AIrisu-072/knowledge-platform/commit/44e1b41219a77809f82fe22045cb4fceaf0c1ed8) をGitHub Actionsの使い捨てPostgreSQLと実Chromiumで検証した。2名の保存・文書参照・提出・引受け・snapshot、2つのHTTP server再起動後の復元、private非開示、transaction rollback/競合、cleanupがPASS。通常CIと既存Document回帰もPASS。詳細は[完了記録](../superpowers/execution/organization-browser-poc-slice-status.md)を参照。

## 現在の範囲

起動時固定の模擬ユーザー（既存の2名、および後続sourceでは6名）を使い、タスク一覧・詳細、privateな文案の保存、共有Document参照、提出、事務担当の引受けと提出内容の閲覧、理由付き差戻と新試行での再提出、Documentを参照する根拠・候補・人間判断の保存、選択根拠に結び付いた合成Agentの候補作成、最終事務タスクの明示的な完了、担当中タスクの保留と再開を行う。PostgreSQLを状態の正本とし、ページ再読込でも保存済み状態を取得する。未保存の入力と結果不明操作はタブ内メモリーに保持する。

これは認証システムではない。各loopbackポートへ接続できる利用者はその固定profileとして扱われる。顧客情報・秘密情報・production DBを使用しない。外部公開・production deploy・Tauri実行を含まない。

## 準備

既存Document PoCと同じLinux/Rust/Node/PostgreSQLの資格・実行権限がある環境を使う。既知のsocket/browser拒否を別ポート・別経路・別環境で迂回しない。

1. repositoryの固定toolchainを準備し、`pnpm install --frozen-lockfile --ignore-scripts` と `pnpm --filter @knowledge-platform/document-web build` を実行する
2. `cargo build --locked -p organization-server -p document-semantic-inspection-worker -p document-diff-worker` を実行する。必要な既存worker/native要件は [Document PoC運用](document-poc-runtime-v0.md) に従う
3. 新しい使い捨てPostgreSQL DB、空の専用storageディレクトリを用意する。既存Document PoC DBのpolicyを上書きしない
4. 同じDB・storage・built GUIを2profileで共有し、次の環境変数を明示する。DB URLやstorage pathをrepositoryへ保存しない

```sh
export KP_RUNTIME_MODE=organization-synthetic
export KP_ORGANIZATION_PROFILE=sales-01
export KP_DATABASE_URL='postgres://…/organization_poc'
export KP_STORAGE_ROOT='/absolute/path/to/disposable-storage'
export KP_DSI_WORKER='/absolute/path/to/document-semantic-inspection-worker'
export KP_DIFF_WORKER='/absolute/path/to/document-diff-worker'
export KP_WEB_DIST='/absolute/path/to/apps/document-web/dist'
# Document PoCが必要とする場合だけ、資格取得済みPDFiumの既存指定も設定する
# export KP_DSI_PDFIUM_RUNTIME_DIR='/absolute/path/to/qualified-pdfium'

./target/debug/organization-server migrate
./target/debug/organization-server bootstrap-poc
./target/debug/organization-server serve
```

`migrate` は明示コマンドだけでDocument既存migrationとWork専用schema/migration ledgerを処理する。通常の `serve` はmigrationもseedも実行しない。`bootstrap-poc` はDocumentの既存bootstrap portでsales-01のfixture作成権限、office-01と固定Document provider poc/poc-agentのread/readHistoryを初期化する。Agent対応前のDBを含め、既存policyが異なる場合は停止し、暗黙にgrantを追加しない。新しい合成Agent PoCでは新しい所有された使い捨てDBを用いる。

## 共有文書を用意する

タスクの入力文書はDocumentのGUIで作成・公開する。WorkはDocumentのACLや版を変えない。既存の公開文書を2profileで読めるならそのIDを利用できる。

初めての使い捨てDBでは、salesサーバー起動後に次の手順を使う。

1. ブラウザーを開くPC側に、実在する顧客情報を含まないUTF-8の `organization-poc-reference.txt` を作る。本文の例は `【合成データ】PoCの共有参照資料です。実在する顧客情報を含みません。`。SSH転送時もファイルは手元PC側に用意する
2. 営業のOrganization画面の「文書」で「System Root」を選び、「文書を登録」を押す。「PoC Shared」は別のDocument PoC fixtureであり、この手順の登録先ではない
3. 登録先を確認し、文書名 `PoC共有参照資料` と上の原本1件を指定して「下書きとして登録」を押す。登録成功後、authoring用途の「版・改訂」へ自動遷移する
4. 「公開する」を開き、「今すぐ公開」と「公開対象の版とファイルを確認しました。」を選ぶ。画面の「公開する」に続き、「公開を確認」ダイアログの「確定する」を押す
5. 公開成功後、「概要」の「記録・技術情報を確認」に表示される `Document ID` を控える

初回登録の結果が不明なら「登録結果を確認」で照会し、再POST・再登録はしない。初回登録には操作IDが無く、WORKING保存の固定要求再送とは異なる。照会できなければ「編集作業」の一覧や管理者に確認する。

公開・予約公開の確定結果が不明なら、未公開や旧公開維持と断定せず、確認ダイアログの「同じ内容で再試行」で同じ操作ID・同じ対象・同じ要求を再送して結果を確認する。要求は公開画面の一時状態に保持されるため、版の変更、公開方法・予約日時の変更、公開画面の開き直し、画面からの離脱、ページの再読み込み、タブ終了を避ける。既に元の要求を失った場合は新しい公開要求を送らず、管理者に元の操作結果を確認する。公開成功前にWork fixtureを作らない。

Root直下作成、フォルダーの続き表示、選択親への子作成、改名・移動、属性3項目/未読/作成日時の絞り込み、既存WORKINGへ戻る入口、属性編集、既存複数原本の選択差替え、予約取消、取下げ・公開終了は[文書GUI手順](document-gui-v0.md)を参照する。初回登録自体は単原本である。

サーバー側の別shellで同じ環境変数を設定して次を実行する。[Linux手動導入](linux-manual-installation.md)に従っている場合は、同手順の節6で `runtime.env` を読み込み、Document IDを入力して `seed-work` へ渡す。ブラウザー側PCからこのコマンドを実行しない。

```sh
export KP_ORGANIZATION_DOCUMENT_ID='<上で公開したdocumentId>'
./target/debug/organization-server seed-work
```

このコマンドは既存Document read serviceで2profileの現在のPublishedアクセスを確かめてからWork fixtureを作る。既存Workの進捗はリセットしない。入力文書を変えるには新しい使い捨てDBを用意する。差戻対応のfixtureは新しいdefinition versionを使う。以前のforward-only定義のDBは0002 migration後も元の定義を保ち、差戻対応へ自動付替えしない。新しい差戻PoCには新しい使い捨てDBを用いる。

予約公開schedulerはこのOrganization手順では起動しない。予約取消GUIの提供は自動公開の稼働確認ではない。[Document PoCのscheduler起動例](document-poc-runtime-v0.md#scheduler-and-other-boundaries)は `KP_RUNTIME_MODE=poc` 用で、`organization-synthetic` のrequesterを解決しないため流用しない。

## 2名で一連の操作をする

- 営業: `http://127.0.0.1:8090/tasks?view=context`
- 事務: 同じDB/worker/storage/GUI設定で `KP_ORGANIZATION_PROFILE=office-01` として別processを起動し、`http://127.0.0.1:8091/tasks?view=queue`

1. 営業でタスクを選び、文案を保存する。事務の一覧にはまだタスクもprivate本文も出ない
2. 営業で「文書・比較」から入力文書を選び、公開改訂・内容の版・原本一覧を確認する。「原本を取得」で共有資料をダウンロードできる。詳細・比較は従来のDocument画面へ移動し、戻るとタスク選択を保持する
3. 営業で保存済み文案を確認して「提出」を確定する。サーバーの成功応答までPending。応答を確認できない時は同じoperation IDで結果確認する
4. 事務で再読込すると担当待ちタスクが出る。「担当を引き受ける」後、提出時点で固定された本文を読める。元のprivate artifactへの直接アクセスは許可しない
5. ページ再読込で保存済み状態を確認する。履歴・snapshot・次task・operation・必須event stagingは同一Work transactionで保存する

## タスク内で公開入力文書を確認する

営業・事務のどちらも、担当タスクの「文書・比較」で「確認する入力文書」を選ぶ。表示するのは既存Taskに結び付いた共有Documentだけであり、Work-private文案や新しい添付の保存先ではない。

- 公開改訂と内容の版（Version）を別々に表示する。文書の詳細・改訂履歴・比較は既存Document画面へのリンクを使う
- 原本を明示的に取得してもTaskの文案保存・提出・状態変更は行わない。未保存入力を保持し、原本を画面内で実行しない
- 公開改訂が無い、権限を失った、または取得できない時は、以前の文書情報や原本を表示し続けない。「公開文書を再読込」で現在の状態を確認する。authoring/historyへ自動的に切り替えない
- 保留中・完了後の正当な読取りでも同じDocument現在認可を使う。Taskの閲覧権限だけで文書の権限が付与されることはない

この追加経路の受入状況は[タスク内Document参照の状況](../superpowers/execution/organization-document-context-slice-status.md)を参照する。元の入力文書やACLを変更せず、ファイルアップロード・添付追加・新規binding作成は提供しない。

## 差戻して再提出する

1. 引受け済みの事務タスクで差戻理由を入力する。空白だけ・UTF-8で8KiB超の理由は使えない。確認画面のキャンセル/Escapeは送信せず、理由を保持する
2. 差戻を確定すると、事務の試行1は完了したままになる。事務画面には確定した指示を表示し、営業の新private文案へ切り替えない
3. 営業で同じタスクを再読込し、readyの試行2を引き受ける。確定理由と旧提出を読んで、新しいprivate文案を保存する。旧提出本文を上書きしない
4. 再提出すると新しいsnapshotと、同じ事務タスクの新しいready試行ができる。事務で改めて引き受けると新提出を閲覧できる。旧snapshot/理由は不変である
5. 通信結果が不明なら元のoperation IDで確認・再送する。同じtask IDでも古い試行の結果で現在の試行へ巻き戻さない

## 根拠・候補・人間判断を残す

1. 担当中のタスクでContext Surfaceの「根拠」を選ぶ。営業型・事務型のどちらでも同じ操作を使う
2. タスクに結び付いた共有Documentの現在の公開改訂と原本を選び、人間が確認した該当箇所を記載して登録する。本文は複製せず、改訂・版・原本の固定参照を保存する。人間の箇所説明は原本から検証済みの抽出結果ではなく、coverageはunknownと表示する
3. 1件以上の根拠を選び、候補の主張を登録する。候補と原本の事実は別の記録である
4. 正確な候補revisionへ「採用」「修正」「却下」の判断を残す。修正時は採用する主張を入力する。元候補・根拠・以前の判断は書き換えず、判断だけで提出や差戻は実行しない
5. 営業から提出する場合は、共有する根拠・候補・判断を明示的に選び、提出確認で参照集合を確認する。候補の根拠、判断の候補と根拠も選択集合へ含める。未選択recordは提出されない
6. 事務が引受けた後は、受領した選択recordを読み、自分の現在の試行で別の判断を残せる。事務の判断を過去の営業snapshotへ書き戻さない。新しい差戻試行のprivate記録は以前の提出と混ぜない

このPoCでは、現在の試行と受領内容を合わせた根拠・候補、および各候補の可視判断はそれぞれ16件まで。上限では新規登録を拒否し、既存記録を一覧から切り捨てない。collection APIは完全集合1page、limit省略時50・指定は16–100、cursorは未対応。

新規登録は現在公開版のAUTHORITATIVE原本だけが対象。保存後に新しい版が公開されても、過去の根拠は元の改訂・版・原本を指す。原本を開く時やWorkから返す時はDocumentの現在の権限を再確認し、別の版/ファイルへ自動置換しない。閲覧できなくなった場合は古い表示を残さず、権限/利用可否を表示する。Workの提出はDocument権限を変更しない。

入力と結果不明操作は同じ利用者・担当・タスク・試行のタブ内状態として扱う。通信結果不明時は元operation IDで確認・再送し、新しいIDで重複作成しない。タブを閉じると未保存入力は失われる。

## 合成Agentから人間判断へつなぐ

この経路のexecutorは固定規則の検証用処理で、実LLMではない。原本本文の読解・要約・事実検証はしない。Documentの現在認可は実Applicationサービスで確認するが、MCP transportは実行していない。

1. 担当中のタスクで「根拠」から少なくとも1件の参照を登録しておき、「Agent」を開く
2. 目的と、使わせる既存根拠のexact revisionを1–16件選択して依頼する。選択は権限を広げず、他のprivate文案や別タスクを自動追加しない
3. サーバーへ保存された実行IDと状態を確認する。実行中の取消は今後の処理を止めるもので、既に成功した候補を消したりworkflowを戻したりしない。通信結果が不明なら元operation IDで回復し、新しいIDを作らない。回復できたものがqueued/runningの受付記録だけなら、実行が進行中と断定せず、結果不明の表示から状態の再確認または取消を行う
4. 成功後の構造化結果から候補を確認する。作成者organization-synthetic/agent-01、依頼者、Document provider poc/poc-agent、合成実行と本文分析なしの表示を区別する。候補は通常のFindingとして保存され、chatだけには残らない
5. 「根拠」で人間が候補を採用・修正・却下する。Agentは人間判断や提出を代行しない。次担当へ共有するには既存の提出確認で根拠・候補・判断を明示選択する

同時実行はtaskごと1件、現在attemptの実行履歴は最大16件。task/attempt/責任変更、取消、原本権限の喪失後は古い実行結果を新しいcontextへ表示しない。process再起動時の未完了実行はoutcome_unknownとなり、自動再実行しない。完了済み候補の内容は書き換えない。

この追加経路の資格は[合成Agentの最新状況](../superpowers/execution/organization-synthetic-agent-slice-status.md)に記録する。以前のEvidence受入だけでAgent実動作を合格にしない。

## 最終事務タスクを完了する

1. 最終事務タスクで受領内容と判断を確認し、「完了内容を確認」を開く。定義された次担当への提出が必要な営業stepには、この操作を表示しない
2. 対象タスク・試行・現在の担当と、完了後は読取り専用になることを確認する。キャンセル/Escapeでは何も確定しない
3. 「完了を確定」で現在の試行を閉じる。新しい担当や提出snapshotは作らず、過去の提出・根拠・判断・Agent結果を保持する。履歴に完了を表示する
4. 読取りは引き続き現在の担当と原本権限で確認する。完了は非公開情報の共有を増やさない。結果不明時は同じ操作IDで確認し、新しい操作として繰り返さない

完了対応は新しいimmutable definition versionのfixtureに限定する。旧DBへWork migration0005を適用しても、既存workflowの定義と進捗を自動変更しない。以前のPoC DBでは完了操作を追加せず、新しい所有された使い捨てDBで開始する。保留/再開は後続の専用definition versionを使う。

この追加経路の資格は[完了sliceの最新状況](../superpowers/execution/organization-complete-slice-status.md)を参照する。合成Agentの既存成功を新しい完了操作の実証とは扱わない。

## 作業を保留して再開する

1. 現在担当中のタスクで「保留内容を確認」を開き、対象の試行・担当と、未保存入力を自動保存しないことを確認する。キャンセルでは何も変更しない
2. 保留を確定すると、同じ試行と担当のまま読取り専用になる。保存済み文案・根拠・判断・過去の提出は残る。未保存入力はこのタブ内だけに残り、ページを閉じると失われる
3. 再開の確認後に「再開を確定」すると、同じ試行の作業へ戻る。タブ内に残した入力も戻るが、新たな保存・提出は別の明示操作で行う
4. 保留前の古いAgent出力を新しい状態へ採用しない。再開してもAgentを自動再実行しない。過去の保存済み結果は現在権限で確認する

保留/再開は新しいfixture definition versionでのみ提供する。旧workflowの定義・担当・進捗をmigration0006で自動変更せず、新しい所有された使い捨てDBで試す。状態文字列ではなく、serverが返す現在の操作可否と定義action IDを使う。正確な同operation再送・記録の回復は保留中も読取りとして扱い、新しい変更操作とは区別する。

実受入の状況は[保留/再開sliceの最新状況](../superpowers/execution/organization-hold-resume-slice-status.md)を参照する。以前の完了操作の成功を新機能の資格へ付け替えない。

## 複数の担当者・役割・委任を使う

この節は、組織単位・役割・正式割当・期限付き委任・担当変更を追加した後続source向けである（上の固定pinには未収録）。凍結済み設計への接続の具体化は[複数担当の実装追補](../superpowers/specs/2026-10-07-organization-multi-principal-amendment.md)、検証状況は[複数担当の状況](../superpowers/execution/organization-multi-principal-status.md)を参照する。

### 6名の合成profile

| profile | 既定port | 正式割当（役割@組織単位） |
|---|---|---|
| `sales-01` | 8090 | 営業@営業店 |
| `office-01` | 8091 | 事務処理@事務 |
| `review-01` | 8092 | 審査@融資審査 |
| `approver-01` | 8093 | 承認@承認、業務管理@承認 |
| `multi-role-01` | 8094 | 事務処理@事務、審査@融資審査（兼務） |
| `delegate-01` | 8095 | なし（委任を受ける） |

各profileは別processで起動する。利用者はprocess起動時の `KP_ORGANIZATION_PROFILE` だけで決まり、画面・header・query・本文で切り替えることはできない。「業務管理」は合成fixtureの管理担当であり、実組織の管理規則ではない。

### 準備と起動

1. 新しい使い捨てDBで `migrate`、`bootstrap-poc`（sales-01）を実行する。新しいDBでは追加4名にも共有入力文書の閲覧（Read/ReadHistory）だけを付与する
2. 6profileを同じDB・storage・built GUIで起動する。portを変える場合は `KP_BIND` を指定する

```sh
for profile in sales-01 office-01 review-01 approver-01 multi-role-01 delegate-01; do
  KP_ORGANIZATION_PROFILE="$profile" ./target/debug/organization-server serve &
done
```

3. 「共有文書を用意する」の手順でDocument IDを用意し、`seed-work` を実行する。`seed-work` はWorkの業務fixtureと合成Organization policy（組織単位・役割・正式割当）を作る。既存の行は上書きしない

既存DBの更新：migration 0007を適用した後、同じDocument IDで `seed-work` を再実行するとpolicyだけが追加される（既存の進捗は変えない）。以前の2名用 `bootstrap-poc` で初期化したDBはそのまま受け入れるが、追加4名には共有入力文書の閲覧権限がなく、文書の参照は「利用できない」と表示される。追加4名で文書も確認する場合は新しい使い捨てDBを使う。

### 操作の流れ

1. 画面上部のヘッダーに実際の利用者と「担当」（役割@組織単位）が表示される。兼務者は「表示する担当」で一覧の範囲を切り替えられる。切替は表示範囲だけを変え、権限は変えない。現在有効な担当がない利用者には「現在有効な担当はありません」と表示される
2. 「担当・委任を確認」から担当と委任の画面を開く（主ナビゲーションには追加しない）
3. 委任：自分の正式割当を選び、受任者・任せる操作・期限（日本時間、この時刻を含まない）・理由を入力して「委任の内容を確認」→「確定」。担当変更と担当・委任の管理は委任できない。再委任はできない。委任は本人だけが作成でき、委任者ごとに16件（取消・期限切れを含む）まで
4. 委任の取消：「この委任を取り消す」→「確定」。受任者は次の閲覧・操作から、その委任で担当していたタスクの非公開内容を読めなくなる。期限を過ぎた場合も同じである
5. 担当可能な（担当候補の）利用者の一覧には、工程名・状態・「担当を引き受ける」だけが表示される。引受けが確定するまで本文は表示しない。同じタスクを同時に引き受けた場合、確定するのは1名だけで、もう1名には競合が表示される
6. 担当変更（approver-01）：タスク一覧で対象を選ぶと「担当の管理」に現在の担当者だけが表示される（非公開本文は表示しない）。「担当変更の内容を確認」で、工程の役割を持つ現在有効な割当・委任から新しい担当者を選び、理由を入力して「担当変更を確定」。同じ試行の保存済み文案は新しい担当者へ引き継がれ、元の担当者は以後読めない。管理担当は自分自身へ担当変更できない（自分が担当可能なら「担当を引き受ける」を使う）。差戻し後の試行を引き継いだ担当者は差戻指示と差戻し前の提出を読める
7. 正式割当の追加・取消（approver-01）：「担当と委任」画面の「割当を追加」「この割当を取り消す」。自分自身への割当は追加できない。取消は過去の操作記録を書き換えない。取り消した割当で担当中のタスクは自動では解放されず、「担当の責任が終了」と表示されるので、担当変更で解消する。その割当を元にした委任も無効になる
8. 結果が分からない操作は、画面の「同じ操作の結果を確認」で同じ操作IDの記録を照会する。記録がまだ無い場合だけ「同じ操作を再送」が表示される

判定はすべてサーバーが行う。画面の表示・非表示、ボタンの有無、検索結果は許可ではない。時刻の判定はサーバーのUTC時刻で、期限ちょうどの時刻から無効になる。

### 実行確認

`mise run organization:poc:runtime` は、既存の2名の確認に続けて、別の新しいDBで6profileを起動する。実画面で割当の追加、担当変更（非公開文案の引継ぎと旧担当者の非開示）、提出、期限付き委任、2名の同時引受（1件だけ成立）、担当変更、委任と割当の取消、完了を行い、6processを再起動した後に記録・操作結果の回復と非開示を確認する。

## 未対応と検証限界

- Tauri/実Windows/WebView2/native Workspace、ファイル添付、実LLM/外部model・MCP通信、検索の接続は今回の最小slice外
- 理由はUTF-8で1024バイト以内（改行・タブ以外の制御文字は不可）。開始日時は過去へ遡れない
- 役割・割当・委任は合成fixtureの範囲。管理担当の範囲を組織単位で分けること、委任の最長期間、自動の担当解放、管理画面での役割定義の編集は扱わない
- Work fixtureは2stepの1workflow。物理DBでは1aggregateをrow lockし、privateなschema-bound textを保存する。一般workflow designerや大規模運用を意味しない
- AuditはWork transaction内のstagingまで。別Audit pipeline配送の資格取得は主張しない
- 実PostgreSQLとbrowserの確認は、明示承認されたGitHub Actionsの使い捨て環境で行う。ローカルの既知DB/browser拒否を再試行しない。以前、純粋試験と誤認したDocument Node試験が合成loopback listenerを起動した事実は報告・終了確認済みで、実DB資格や追加実行許可を意味しない。純粋テスト/HTTP oneshot/型検査/buildの成功で実runtime合格としない

## 最小開発確認

```sh
cargo test --locked -p organization-server -p work-domain -p work-application -p work-api-http -p work-repository-postgres
cargo test --locked -p document-server --test config_identity
pnpm --filter @knowledge-platform/document-web test
pnpm --filter @knowledge-platform/document-web build
pnpm organization:api:lint
```

PostgreSQL transaction試験は既定で明示ignoreされる。実行していない試験を合格件数へ加算しない。適切な実行権限を持つ使い捨て `*_work_poc_test` DBが用意できた場合に限り、`WORK_POC_TEST_DATABASE_URL` を指定して `cargo test -p work-repository-postgres --test postgres_transaction -- --ignored` を実行する。

## Hostedでの最小実操作確認

`mise run organization:poc:runtime` は既存Document CI後段向けの単発確認である。外部DBを受け付けず、既存と同じ公式PostgreSQL一時containerを別途所有し、独立したtransaction試験用DBとbrowser用DB・storageを作る。既存固定Chromiumでsales/officeの操作を行い、2processを停止・再起動して保存状態を確認した後、所有containerを削除する。

通常CIの成功だけでなく、このOrganization専用stepのtransaction/journey/restart/persistence/shutdown成功を確認して初めて、この最小経路の実runtime検証済みとする。初回PoCの実証は[PR54](https://github.com/AIrisu-072/knowledge-platform/pull/54)のsource `44e1b412` で完了している。差戻追加経路は[PR56](https://github.com/AIrisu-072/knowledge-platform/pull/56) exact `cf28175d` で全CIと実DB/2名browser/両HTTP server再起動後復元/cleanupが成功した。根拠・候補・判断は[PR57](https://github.com/AIrisu-072/knowledge-platform/pull/57) exact `d383bacc` で実DB/2名操作/両HTTP server再起動後復元/cleanupと全CIが成功した。合成Agentは[PR60](https://github.com/AIrisu-072/knowledge-platform/pull/60) exact `48ae1bfd` で実DB/2名操作/両HTTP server再起動後復元/cleanupと全CIが成功した。最終事務の完了・保留/再開・Document原本取得を含むPR67時点の統合結果は、本書冒頭の過去受入記録を参照する。現在pinのmain自身の資格は、冒頭の対象ソースと確認状況で確認し、過去の公開製品headの資格とは分ける。PostgreSQL processそのものの再起動は確認対象に含めていない。画像・trace・videoはoff、raw実行ログ・標準runnerの原文は一時workspace内に保持し、公開artifactは追加しない。既存の有限stage/statusと許可された操作名だけをCIへ出力する。

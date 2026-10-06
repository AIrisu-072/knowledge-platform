# Linuxサーバーへの手動導入と復旧

## この手順でできること

所有者が自分でLinuxサーバーへ導入するための手順。対象は10月2日に設置したPCで、確定情報はCore Ultra 9 285KとLinux方針のみ。ディストリビューション、OS版、メモリー、ディスク、接続先は未確定である。「最新安定版」だけからUbuntu等を選定済みとは扱わない。

**本書の対象は、架空データだけを使うOrganization Browser PoCの導入である。本番利用開始の手順は未完成。** 固定の営業・事務profileを使い、そのポートへ接続した人は同じprofileとして扱われる。認証画面、実利用者の識別、production modeはない。実文書・顧客情報を投入せず、インターネットや社内LANへ公開しない。

- 導入対象の資格：固定合成2profile・画像保存なしのUbuntu機能受入に合格したmain。対象PCでの手順実行、本番認証、見た目全体の比較検証は対象外。固定SHAと受入記録が未確定の版は実行しない
- 固定ソース：[PR91](https://github.com/AIrisu-072/knowledge-platform/pull/91)統合main `933d3b0f894e610496022defae8e494b16de39ea` / tree `8c6789bc3ae0332894ab1dea8f1b84686444a611`。PR公開headの資格と、このmain自身のpush CIの結果を区別する
- 採用sourceは旧pin e249までの初回登録・公開/WORKING・Folder操作・属性/未読/日時条件・文書移動・正式改訂の続き表示を保持し、[PR90](https://github.com/AIrisu-072/knowledge-platform/pull/90)比較結果の続き表示、[PR92](https://github.com/AIrisu-072/knowledge-platform/pull/92)通常詳細からの閲覧専用コンテンツ版履歴/旧原本、PR91イベント履歴の続き表示と両履歴の共存を含む。これら3機能は旧固定版e249には含まれない
- main資格の最終確認：2026-10-06 21:24:51 UTC。[main自身のpush CI37530751555](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37530751555)はattempt1・全13jobs/13checks成功、failure/skip/未終端0、終端後公開artifact0。このmainに対応するworkflowはpush CIの1runであり、PR91側の4runs/18checksや旧pinの結果を転用しない。終端後のmain ref/tree/両parents（旧main f431c374・PR91 head63ab9728）も一致した
- [実runtime job112499205642](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37530751555/job/112499205642)の公式stepでcheckout、`Real composition-root acceptance`、`Real Organization two-principal acceptance`、`Emit bounded runtime evidence`の成功を確認した。Document stepは21:09:45、Organization/summaryは21:11:22、jobは21:11:25 UTCに成功終端した
- 同runtimeの元の接続済みlog読取は初回にTransport closedとなり、stdout本文は未取得。actual head/clean/qualifiedの印字、GUI件数、実測browser件数、各case/cleanup receipt、run UUID・port・artifact/DB/storage hashを直接読取済みとは扱わない。有限summaryでどの個別行が省略されたかも未観測である。追加ログ取得・別route/credential・再実行で補っていない
- 必須runtime gateは、同じ固定sourceが強制する終了条件と今回mainの成功stepの対応から合格と評価した。[固定workflow](https://github.com/AIrisu-072/knowledge-platform/blob/933d3b0f894e610496022defae8e494b16de39ea/.github/workflows/ci.yml)はpushでmainの`github.sha`をcheckoutし、[Document runner](https://github.com/AIrisu-072/knowledge-platform/blob/933d3b0f894e610496022defae8e494b16de39ea/tools/document-poc-runtime/run.mjs)と[summary](https://github.com/AIrisu-072/knowledge-platform/blob/933d3b0f894e610496022defae8e494b16de39ea/tools/document-poc-runtime/ci-summary.mjs)が同head/clean・実production build・全必須工程・Agent provenance・同じowned DB/storageでのHTTP再起動同一性を検査する。失敗は非zeroへ伝播し、qualified述語を満たさなければsummaryも失敗する。値の直接観測やPR実行結果の転用ではなく、既存合格条件も変更していない
- Document22工程/選択journey18＋persistence5、Agent9 groups、Organization8工程/選択2＋2は固定source構成と今回の成功実行からの対応推論であり、stdoutの実測件数ではない。summaryのqualified述語はbrowser件数やskipped=0を直接検査しないため、no-skipの対応評価は今回の固定選択source/configにskip/only/expected-failure経路がない範囲に限る。GUI1312件/53 suitesは同sourceのローカルbaselineであり、今回hostedの印字値は未読である
- [Rust実DB job112499205731](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37530751555/job/112499205731)の新しい正式ログではactual checkoutがmain933d自身。workspace1849成功/既存skip10、追加21成功/skip0、別追加7成功/既存skip1を直接確認した。指定実DB36/36とFolder回帰4/4はそれぞれ各1本の新しいPASS行へ一致し、missing/ambiguous各0。個別state値は同tree assertionとPASSの対応であり、全値の直接ログではない
- Documentのowned cleanupはrunner、Organizationのbuild/database/transaction/initialize/journey/restart/persistence/shutdownとowned cleanupは[固定Organization runner](https://github.com/AIrisu-072/knowledge-platform/blob/933d3b0f894e610496022defae8e494b16de39ea/tools/organization-poc-runtime/run.mjs)の失敗伝播と今回成功stepからの対応推論。個別PID/CIDや`owned-container-removed`文字列は今回は未読である。環境は既存PostgreSQL18.6・固定toolchain・合成2profile・画像なしUbuntu PoC。HTTP再起動をPostgreSQLプロセス再起動へ読み替えない
- 両履歴の通常表示・明示再読取・旧Version 1の原本hash・HTTP再起動後の読取、比較の通常表示・終端・再読取、従来の文書移動/正式改訂/本人・Agent readState保持は、同treeの選択sourceと成功実行の対応で確認する。個々のHTTP応答やassertion値の直接公開ログではない。コンテンツ版は通常2版/再起動後3版で、該当旧版の原本は1件に限る
- イベント履歴101件目、コンテンツ版101件目/実複数旧原本、比較結果50件超、正式改訂100件超の実GUIは未資格。DOM100+1/50+1や既存HTTP pageSize=1試験をその代替にしない。現在開発中の履歴一覧から旧版・原本・イベントを開く通常入口はこの固定版に未収録で、追加される操作説明は後続source向けである
- WORKING固定再送の実通信資格は成功応答body途中喪失だけで、status/headers全喪失は未資格のままである。資格対象は画像なしUbuntuの実操作PoC。macOS golden比較は未実行・未更新。影響候補Mock 2・3・4・7の4枚に加え、他3枚の画素不変も未証明で、全visual資格は主張しない
- この手順そのものの対象PCでの実行、常設DBのbackup/restore、PostgreSQLプロセス再起動後の確認は未実施。CI成功と区別する
- GPU、CUDA、外部モデル、Tauriは使わない。Agentは固定の合成executorであり、既存Document現在認可を確認して候補を作る。本文分析・実LLM・外部MCP通信は行わない
- 本書のコマンドは所有者が実行する。既存本番サーバーへの接続や秘密情報の送信を代行するものではない

### 過去の受入記録

以下は以前の固定版に対する記録であり、上記の最終ソースや対象PCの手動導入へ資格を付け替えない。

#### 2026-10-06 17:06 UTC PR89統合mainの固定版

以下の「今回」「未収録」は当時のpinと比較GUI開発時点を指す。

- 導入対象の資格：固定合成2profile・画像保存なしのUbuntu機能受入に合格したmain。対象PCでの手順実行、本番認証、見た目全体の比較検証は対象外。固定SHAと受入記録が未確定の版は実行しない
- 固定ソース：[PR89](https://github.com/AIrisu-072/knowledge-platform/pull/89)統合main `e249fb8da91549115d1371c05959e3219dbfde1c` / tree `c8188d99b33b52ce36383c96d19e0d9f39fcb92c`。PR公開headの資格と、このmain自身のpush CIの結果を区別する
- 採用sourceはPR69初回登録〜PR74複数原本編集・固定要求再送・「編集作業」入口、PR76 Root直下作成、PR78フォルダーの続き表示、PR79選択親への子作成、PR80改名、PR81属性3項目、PR82未読条件、PR84作成日時条件、PR87フォルダー移動を保持し、PR88文書移動とPR89正式改訂の続き表示・明示比較選択の保持を含む。旧固定版 `cd6aafcc` にはPR88/89は含まれない
- main資格の最終確認：2026-10-06 17:06:10 UTC。[main自身のpush CI37497603490](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37497603490)はattempt1・全13jobs/13checks成功、failure/skip/未終端0、終端後公開artifact0。このmainのpush workflowは1runであり、PR89側の4workflow/18checksとは別の実行である。終端後のmain ref/tree/両parents（旧main ea406848・PR89 head075a1e79）も一致した
- [実runtime job112386056459](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37497603490/job/112386056459)の正式stdoutでactual head=e249fb8、gitDirty=false、acceptanceQualified=true、Document全22工程、journey18件＋HTTP再起動後persistence5件、Agent9項目/provenanceVerified=true、restartIdentityVerified=trueを確認。GUIも同mainログの47 suites/1161件PASSであり、PRのローカル1160件や後続featureの件数を流用しない
- [Rust実DB job112386055919](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37497603490/job/112386055919)もmain e249自身のcheckout。workspace1849成功/既存skip10、追加21成功/skip0、別追加7成功/既存skip1。指定実DB36/36とFolder回帰4/4を各1本の正式PASS行で照合し、missing/ambiguous各0。個別state値は同tree assertionとPASSの対応であり、全値の直接ログとは扱わない
- journeyのmetadata個別PASSは直接公開されている。一方persistenceの有限tests配列はtruncated=trueでmetadata個別行を省いており、再起動後の文書移動replay・正式改訂read/比較・snapshot/readState保持は5/5 phase PASSと同treeの選択sourceを対応させた推論である。metadata個別PASS行を直接観測したとは記録しない
- Organizationは同runtime jobでbuild/database/transaction/initialize/journey/restart/persistence/shutdownの8phaseと `cleanup: owned-container-removed` を直接stdoutで確認。既存journey2＋persistence2の個別操作・HTTP2 process再起動後の内容は同tree sourceとphase PASSの対応推論。Document cleanupも最終qualified/passと同sourceのcleanup失敗条件からの推論であり、個別PID/CIDの生receiptはない。HTTP再起動をPostgreSQLプロセス再起動へ読み替えない
- 環境は既存PostgreSQL18.6・固定toolchain・合成profileによる画像なしPoC。WORKING固定再送の実通信資格は成功応答body途中喪失だけで、status/headers全喪失は未資格のままである
- 文書移動は同権限の合成Shared→Sandboxを1回操作し、正式改訂・Version・原本hash・本人/Agent readStateを保持する範囲。正式改訂は既存2件の通常表示、明示した1.1→1.0の比較、一覧の先頭再読取後の同pair新POSTとHTTP再起動後の表示が対象である。これらの個別assertionは同tree sourceと正式PASSを対応させた推論であり、個々のHTTP応答・値の直接公開ログではない
- PR89の実GUI100件超は未資格。今回の比較結果の続き表示はこの固定版に未収録で、[文書GUI手順](document-gui-v0.md#比較結果の続きを表示する)は後続source向けである。実GUI50件超も未資格。DOMの100+1/50+1や既存HTTP pageSize=1試験を実GUI資格へ転用しない
- 資格対象は画像なしUbuntuの実操作PoC。macOS golden比較は未実行・未更新。影響候補Mock 2・3・4・7の4枚に加え、他3枚の画素不変も未証明で、全visual資格は主張しない
- この手順そのものの対象PCでの実行、常設DBのbackup/restore、PostgreSQLプロセス再起動後の確認は未実施。CI成功と区別する
- GPU、CUDA、外部モデル、Tauriは使わない。Agentは固定の合成executorであり、既存Document現在認可を確認して候補を作る。本文分析・実LLM・外部MCP通信は行わない
- 本書のコマンドは所有者が実行する。既存本番サーバーへの接続や秘密情報の送信を代行するものではない

#### 2026-10-06 09:11 UTC PR87の固定版

- 導入対象の資格：固定合成2profile・画像保存なしのUbuntu機能受入に合格した公開製品head。対象PCでの手順実行、本番認証、見た目全体の比較検証は対象外。固定SHAと受入記録が未確定の版は実行しない
- 固定ソース：[PR87](https://github.com/AIrisu-072/knowledge-platform/pull/87)の受入済み公開製品head `cd6aafcc4e914050d8fc0e0f85483d82572e29da` / tree `dfba74428ef342d43369b094bd9e5117f3ee9fb4`。基点mainは `b9f447faa294f1898ef2b1d375b055c4b9e96cd8`。このpinの製品資格とmain統合結果は別に確認する
- 採用sourceはPR69初回登録〜PR74複数原本編集・固定要求再送・「編集作業」入口と、PR76 Root直下作成、PR78続き表示、PR79選択親への子作成、PR80改名、PR81属性3項目の絞り込みを保持し、PR82未読条件、PR84作成日時条件、PR87フォルダー移動・同名衝突mapper・既存移動受入を含む。旧固定版0801には未読・日時・今回移動は含まれない
- 製品資格の最終確認：2026-10-06 09:11:48 UTC。固定合成2profile・画像なしUbuntu機能受入の資格であり、main統合結果や対象PC導入の資格とは分ける
- [通常CI37438675289](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675289)は13/13 jobs成功。[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675264)・[Sandbox Preflight](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675307)も成功し、[Organization D2](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675413)は既存適用条件によりskip。全18checksは15成功/既存skip3/failure0、全4runはattempt1で終端。DSIのmacOS qualificationとOrganization D2の2jobsの既存skipを、画像・golden資格へ転用しない
- [実runtime job](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675289/job/112186722085)は公開製品head自身のcheckout、gitDirty=false、acceptanceQualified=true、全22stages passedを示す。通常RustのPR検査用merge `6ea0486f3057b83457cd2711849007e723ddde1d` は同treeで、parentsは基点mainと公開製品head。ローカルGUI952件/42 suites・schema/型/build成功と独立SOURCEレビューGOはローカル資格として区別する
- [Rust実DB job](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675289/job/112186722380)で指定36件を今回の正式PASS行と36/36照合し、新規同名衝突のHTTP/Repository2反例と既存拡張no-op/replay/stale caseもPASS。HTTP409/REVISION_CONFLICT・台帳0、rollbackとFolder/Document/access・台帳/イベント/監査不変は同treeの該当assertionと正式case PASSの対応から確認し、個別state値の直接ログとは扱わない。workspace1849成功/既存skip10、追加21成功、別追加7成功/既存skip1、全failure0
- Organizationは同[実runtime job](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675289/job/112186722085)でbuild/database/transaction/initialize/journey/restart/persistence/shutdownの8phaseがpassed。既存journey2＋persistence2の移動・fresh GET/DOM・201件・通常ナビ往復の詳細は、固定sourceの同2+2 caseとphase成功を対応させた推論である。個別case名・個々のassertion値は公開stdoutに出ていない
- Document18件＋HTTP再起動後5件は全PASS・fail/skip0。属性・未読・日時の実GET/往復と本人/Agent readState不変を含む既存受入を保持し、Agent9項目・provenanceVerified=trueを同[実runtime job](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37438675289/job/112186722085)で確認した。WORKING固定再送の実通信資格は成功応答のbody途中喪失に限る
- Document側HTTP再起動ではrestartIdentityVerified=true。Organizationの2HTTP process再起動後の現在親・同ID/同名/revision 2、元create/rename/move receiptのsales固定replay・office403と前後read不変は同sourceのpersistence caseとphase成功からの推論。HTTP再起動をPostgreSQL process再起動へ読み替えない
- Organizationのcleanupは正式logのowned-container-removedで確認。Document cleanupは最終passed summaryと同sourceのfail-closed cleanup経路からの推論であり、個別PID/CIDの生receiptはない。全4runの終端後公開artifact0を確認した

#### 2026-10-06 03:27 UTC PR81までの固定版

- 固定ソース：最終受入main `0801c9864bdb7faf5fcbe7ee1062367335ee7bfb` / tree `794407fc62e2418bd6ecbdc2de4541cd3e207b02`
- GUI統合の確認：PR69初回登録、PR70取下げ・公開終了、PR71属性編集、PR72予約取消、PR73 WORKING backend、PR74複数原本編集・固定要求再送・「編集作業」入口を保持し、PR76 Root直下作成、PR78フォルダーの続き表示、PR79選択親への子作成、PR80改名、[PR81](https://github.com/AIrisu-072/knowledge-platform/pull/81)属性3項目の絞り込みを含む。PR81公開head `426db3a5a43083a1b4bd76b656ea58322019eed3` / tree `794407fc62e2418bd6ecbdc2de4541cd3e207b02` の[通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37405694808)13/13 jobs、[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37405694768)、[Sandbox](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37405694748)が成功。全18checksは15成功・既存条件skip3・failure0で、Organization専用workflowの既存条件skipを含む全4runの終端後公開artifact0を確認した
- 統合後main自身の[push CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37407454204)は、2026-10-06 03:27 UTC確認で全13jobs/checks成功・failure0・skip0、終端後公開artifact0。今回mainの別実行でGUI788件/39 suites、Document18件＋HTTP再起動後5件、属性3条件の実GET一致/不一致/解除・詳細往復、Agent9項目/provenance、Organizationのbuildを含む全8stages（固定source/configの2+2に対応）、owned cleanupを確認した。Rust1831成功/9skip、別feature suite21成功および7成功/1skip、指定実DB36件の今回PASS名36/36を照合。exact main checkoutとDocument summaryのclean、同treeのOrganization runnerの必須clean gate通過を確認したが、Organization固有のhead/dirtyが公開logへ単独出力されたとは扱わない。環境はPostgreSQL18.6・固定合成2profileで、作業版の固定再送資格は実成功応答のbody途中喪失に限定し、全status/headers喪失は未資格のままとする

#### PR74以前の固定版

- 当時の固定ソース：最終受入main `3d8deb253de19cb0954aa70a9a31cc5c4fc7540c` / tree `3f1ac6aa9e66d58bd5f01316e46334a48a64664f`
- GUI統合の確認：PR69初回登録、PR70取下げ・公開終了、PR71属性編集、PR72予約取消、PR73 WORKING backendを保持した[PR74](https://github.com/AIrisu-072/knowledge-platform/pull/74) exact `ce56801f7ec73ed284a99838f07cfe0c92cf71f4` / tree `3f1ac6aa9e66d58bd5f01316e46334a48a64664f`。[通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37298371770)、[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37298371873)、[Sandbox](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37298371821)。確認結果：required-checkを含む通常CI13/13・DSI・Sandboxが成功。Rust1599成功/9skip、指定実DB36成功、GUI404・runtime補助試験161成功。Document18件とHTTP再起動後5件、Agent9項目/provenance、Organizationの操作・往復・再起動・owned cleanup、公開artifact0を確認した。初回PUT・新版POST・続くPUTで、実成功応答のbody途中喪失から実headers/同一requestの失敗→UNKNOWN→同一要求の明示再送・結果一致・DB snapshot不変を確認。status/headersも全喪失する旧faultのGUI明示再送は未合格のままで、今回へ付け替えない
- 統合後mainの[push CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37301558995)。確認日：2026-10-05 11:31 UTC。確認結果：main自身のpush CIでrequired-checkを含む13/13 jobsが成功。Rust1599成功/9skip、指定実DB36成功。Document18件とHTTP再起動後5件、Agent9項目/provenance、Organizationの通常ナビ往復・操作・再起動・owned cleanup、公開artifact0を、PRとは別のmainログで確認した。exact head/clean、PostgreSQL18.6、固定合成2profileを照合した。作業版の固定再送資格は実成功応答のbody途中喪失に限定する

- 当時の固定ソース：統合済みmain `6c514850850110a3c2f8b2b5664ec263510c5d47`。受入済み[PR67](https://github.com/AIrisu-072/knowledge-platform/pull/67) `a39c90c2` と同一tree `880b1a57abc6890ed47df5e7bc16a4694d4546cc`
- 当時の確認：PR67の[通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37251574840)、[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37251574859)、[Sandbox](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37251574842)成功。同treeで使い捨てPostgreSQL・2名の合成Agent/完了/保留再開/原本取得・HTTPサーバー再起動後の復元・cleanupを確認済み。統合後mainの[push CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37253316995)もrequired-checkを含む13 jobsと実受入が成功

## 1 開始前の確認

- [ ] `/etc/os-release`、`uname -m`、`uname -r`、空きディスクを確認した
- [ ] Linux x86_64で、既存DSI/DiffのLandlock・seccomp保護が動作する。CIの参照OSはUbuntu 24.04だが、選定OSの資格取得を意味しない
- [ ] 専用の非root Linuxユーザーを使い、そのユーザーだけが下記作業ディレクトリを読める
- [ ] Bash、Git、curl、tar、OpenSSL、C/C++ビルド環境、pkg-config、Fontconfig、mise/rustupをOS・各提供元の公式手順で導入した。ディストリビューション未定のためapt/dnf等のコマンドはここでは固定しない
- [ ] 既に許可されたDocker Engineを利用でき、`docker info` が成功する。Docker権限は強い権限であり、動作させるためだけにユーザー追加や保護設定変更をしない
- [ ] 127.0.0.1の15432、8090、8091番portが未使用。別の既存サービスを停止して流用しない
- [ ] 既存DB、既存storage、SearchのDBを再利用しない。中断後にこの初回手順を最初から再実行しない

保護・socket・workerの前提で拒否された場合は停止する。sandbox無効化、root実行、外向きbind、別経路への切替で通過させない。対象OSを確定してから、そのOSで必要なネイティブ依存と実runtime試験を確認する。

## 2 固定ソースとビルド

以降はBash。初回用の専用ディレクトリを作る。既に存在したら停止して中身を確認する。

```bash
set -euo pipefail
set +x
umask 077
export KP_HOME="$HOME/knowledge-platform-poc"
export KP_SOURCE_SHA='933d3b0f894e610496022defae8e494b16de39ea'
[[ "$KP_SOURCE_SHA" =~ ^[0-9a-f]{40}$ ]] || { echo '受入済みの固定SHAが未設定です'; exit 1; }
export KP_SOURCE="$KP_HOME/releases/$KP_SOURCE_SHA"
test ! -e "$KP_HOME"
install -d -m 700 "$KP_HOME" "$KP_HOME/releases" "$KP_HOME/config" \
  "$KP_HOME/storage" "$KP_HOME/backups"
git clone --no-checkout https://github.com/AIrisu-072/knowledge-platform.git "$KP_SOURCE"
cd "$KP_SOURCE"
git checkout --detach "$KP_SOURCE_SHA"
test "$(git rev-parse HEAD)" = "$KP_SOURCE_SHA"
git status --short
```

`AGENTS.md` と `mise.toml` を確認する。miseが設定への信頼確認を求めた場合は、内容を確認して所有者が判断する。固定版はRust 1.98.1、Node 24.21.0、pnpm 12.4.1。勝手に「最新」へ上げない。

```bash
mise install rust
mise install node
mise install pnpm
eval "$(mise env -s bash)"
export CARGO_TARGET_DIR="$KP_SOURCE/target"
PDFIUM_DYNAMIC_LIB_PATH="$(bash experiments/document-semantic-inspection/scripts/install-pdfium.sh)"
export PDFIUM_DYNAMIC_LIB_PATH
pnpm install --frozen-lockfile --ignore-scripts
cargo build --locked -p organization-server \
  -p document-semantic-inspection-worker -p document-diff-worker
pnpm --filter @knowledge-platform/document-web build
test -x "$CARGO_TARGET_DIR/debug/organization-server"
test -d "$KP_SOURCE/apps/document-web/dist"
git diff --exit-code
```

これは受入時と同じdebugビルド経路。releaseビルドの性能・適格性を主張しない。旧0801からPR87までの共通Cargo manifest/lock/deny更新は当時の基点main側に含まれる。旧固定cd6aafcc→main e249の当時の限定照合は履歴として保持する。今回の旧固定e249→main933dの新しいGit object bytes照合では、OrganizationのCLI/env/identity/bootstrap、Document 0001〜0011とWork 0001〜0006のmigration/台帳、Cargo manifest/lock/deny、Rust/Node/pnpmの固定設定とlock、生成SDK/API schema、PDFium取得scriptの指定17objectsが全て同一である。業務schemaのsource不変は対象既存DBの履歴や安全な更新の実証ではない。この照合だけで対象実DBの更新可能性や全面互換を保証しない。採用sourceの `Cargo.lock` を保持し、上記の既存 `--locked` debug buildとGUI buildを行う。旧binary/distの流用、旧lockへの戻し、`cargo update`、移動専用の依存追加やSDK再生成は行わない。PDFiumは既存スクリプトが固定151.0.7881.0のarchiveとlibraryのhashを確認する。失敗した場合は非検証版へ差し替えない。既存Dockerfileはツール/scheduler向けで、Organizationアプリを配備するimageではない。

## 3 専用の合成DBと秘匿設定

この例はlocalhost限定のPoC専用PostgreSQLを新規作成する。DBのpostgres管理者を使用する簡易構成であり、本番の最小権限構成ではない。データは専用Docker volumeへ保持する。Dockerコンテナは自動起動設定にしない。

```bash
export KP_DB_CONTAINER=kp-organization-poc-db
export KP_DB_VOLUME=kp-organization-poc-pg18
export KP_DB_NAME=kp_organization_poc
if docker container inspect "$KP_DB_CONTAINER" >/dev/null 2>&1; then
  echo '同名containerがあるため停止'; exit 1
fi
if docker volume inspect "$KP_DB_VOLUME" >/dev/null 2>&1; then
  echo '同名volumeがあるため停止'; exit 1
fi
db_password="$(openssl rand -hex 32)"
printf 'POSTGRES_USER=postgres\nPOSTGRES_DB=%s\nPOSTGRES_PASSWORD=%s\n' \
  "$KP_DB_NAME" "$db_password" > "$KP_HOME/config/postgres.env"
export KP_DATABASE_URL="postgres://postgres:${db_password}@127.0.0.1:15432/$KP_DB_NAME"
unset db_password
export KP_RUNTIME_MODE=organization-synthetic
export KP_STORAGE_ROOT="$KP_HOME/storage"
export KP_DSI_WORKER="$CARGO_TARGET_DIR/debug/document-semantic-inspection-worker"
export KP_DIFF_WORKER="$CARGO_TARGET_DIR/debug/document-diff-worker"
export KP_WEB_DIST="$KP_SOURCE/apps/document-web/dist"
export KP_DSI_PDFIUM_RUNTIME_DIR="$PDFIUM_DYNAMIC_LIB_PATH"
for key in KP_HOME KP_SOURCE_SHA KP_SOURCE KP_DB_CONTAINER KP_DB_VOLUME KP_DB_NAME \
  KP_DATABASE_URL KP_RUNTIME_MODE KP_STORAGE_ROOT KP_DSI_WORKER KP_DIFF_WORKER \
  KP_WEB_DIST KP_DSI_PDFIUM_RUNTIME_DIR; do
  printf 'export %s=%q\n' "$key" "${!key}"
done > "$KP_HOME/config/runtime.env"
chmod 600 "$KP_HOME/config/postgres.env" "$KP_HOME/config/runtime.env"
docker pull postgres:18.6-bookworm
KP_DB_IMAGE="$(docker image inspect --format '{{index .RepoDigests 0}}' postgres:18.6-bookworm)"
test -n "$KP_DB_IMAGE"
printf '%s\n' "$KP_DB_IMAGE" > "$KP_HOME/config/db-image.txt"
docker volume create --label kp.purpose=organization-synthetic "$KP_DB_VOLUME"
docker run -d --name "$KP_DB_CONTAINER" --label kp.purpose=organization-synthetic \
  --env-file "$KP_HOME/config/postgres.env" \
  --publish 127.0.0.1:15432:5432 \
  --mount "type=volume,source=$KP_DB_VOLUME,target=/var/lib/postgresql" \
  "$KP_DB_IMAGE"
ready=0
for attempt in {1..60}; do
  if docker exec "$KP_DB_CONTAINER" pg_isready -h 127.0.0.1 -U postgres -d "$KP_DB_NAME" >/dev/null; then
    ready=1; break
  fi
  sleep 1
done
test "$ready" = 1
docker exec "$KP_DB_CONTAINER" psql -X -v ON_ERROR_STOP=1 -U postgres -d "$KP_DB_NAME" \
  -c 'SHOW server_version;'
```

versionが18.6であることを確認する。readinessはTCPを指定し、初期化中だけ動くUnix socketの一時serverを合格にしない。PostgreSQL 18の公式imageのvolume先は `/var/lib/postgresql`。古い17以前の例と混同しない。[公式imageの説明](https://hub.docker.com/_/postgres)

2つのenvファイルは秘密情報を含む。Git、チャット、画像、サポート用ログへ貼らない。`env` やcontainer inspectの全内容を公開しない。Docker管理者とOS管理者には参照され得る。ディレクトリ権限はディスク暗号化や本番secret管理の代わりにはならない。

## 4 明示的な初期化

初回の新しい専用DBだけで実行する。Document migration 0001〜0011（0011はOutbox）を `_sqlx_migrations`、Work migration 0001〜0006を別schema/台帳 `work.schema_migrations` へ適用する。Workの0004は合成Agent、0005は完了、0006は保留/再開の記録を支える。`serve` はmigrationやseedを実行しない。

```bash
source "$KP_HOME/config/runtime.env"
export KP_ORGANIZATION_PROFILE=sales-01
unset KP_BIND
"$KP_SOURCE/target/debug/organization-server" migrate
"$KP_SOURCE/target/debug/organization-server" bootstrap-poc
```

DocumentとWorkのmigrationは別々に適用され、両方を一括rollbackするコマンドではない。失敗・結果不明ならDBと台帳を調査し、ledgerの行削除やchecksum変更で通さない。`bootstrap-poc` はsales-01のfixture作成権限、office-01と固定Document provider `poc/poc-agent` のread/readHistoryを作る。異なる既存policyは上書きせず停止する。Agent対応前のDBへ暗黙にgrantを追加しない。

## 5 営業と事務を起動

Linuxサーバーの別々のterminalでforeground起動する。所有者が終了状態を確認できるよう、自動再起動やバックグラウンドdaemon化はここでは追加しない。

営業terminal:

```bash
set -euo pipefail
set +x
source "$HOME/knowledge-platform-poc/config/runtime.env"
unset KP_BIND
KP_ORGANIZATION_PROFILE=sales-01 "$KP_SOURCE/target/debug/organization-server" serve
```

事務terminal:

```bash
set -euo pipefail
set +x
source "$HOME/knowledge-platform-poc/config/runtime.env"
unset KP_BIND
KP_ORGANIZATION_PROFILE=office-01 "$KP_SOURCE/target/debug/organization-server" serve
```

3つ目のterminalでhealthを確認する。200と `{"status":"ok"}` が必要。healthだけで全業務経路を合格とはしない。

```bash
curl --fail --max-time 30 http://127.0.0.1:8090/health/ready
curl --fail --max-time 30 http://127.0.0.1:8091/health/ready
```

サーバー自身のブラウザーでは営業 `http://127.0.0.1:8090/tasks?view=context`、事務 `http://127.0.0.1:8091/tasks?view=queue` を開く。手元PCから見る場合は、既に許可・設定済みのSSH接続で8090/8091をそれぞれ手元の127.0.0.1へ転送する。SSH導入やfirewall変更はこの手順に含めず、serverを `0.0.0.0` へ変更しない。両ポートへの接続は両profileの操作権限を持つことになる。

## 6 合成文書とタスクの準備

初回だけ、営業画面の通常GUIから合成文書を1件登録して公開する。ファイル選択はブラウザーを開いているPCのファイルを使う。SSH転送で手元PCから見ている場合も、Linuxサーバーではなく手元PC側にUTF-8の `organization-reference.txt` を作る。内容は架空の確認文だけにする。

ブラウザー側PCでBashを使える場合の例（同名ファイルがあれば停止する）。Bashを使わない場合は、テキストエディターで同じ本文をUTF-8の `.txt` として保存する。この操作ではサーバー用の `runtime.env` を読み込まない。

```bash
set -euo pipefail
umask 077
test ! -e "$HOME/organization-reference.txt"
printf '【合成データ】2名の動作確認だけに使う共有資料です。\n' \
  > "$HOME/organization-reference.txt"
```

1. 営業のOrganization画面でメインナビゲーションの「文書」を開く
2. フォルダーの「System Root」を選び、「文書を登録」を押す。Organizationの `bootstrap-poc` が権限を用意するのはこのルートであり、別のDocument PoC fixtureの「PoC Shared」は選ばない
3. 登録先が「System Root」であることを確認し、文書名を `PoC共有参照資料`、原本ファイルを上で作成した1件にする。「下書きとして登録」を押す
4. 登録が確認できると、対象文書のauthoring用途の「版・改訂」へ自動で移動する。登録直後はWORKINGの下書きであり、まだ公開されていない
5. 対象版と原本を確認して「公開する」を開く。「今すぐ公開」を選び、「公開対象の版とファイルを確認しました。」にチェックする
6. 画面の「公開する」を押し、「公開を確認」ダイアログの対象を確認して「確定する」を押す。成功表示を確認するまで次へ進まない
7. 公開成功後、同じ文書の「概要」で「記録・技術情報を確認」を開き、`Document ID` を控える。Version IDやrevision番号と取り違えない

初回登録の結果が不明なら、画面の「登録結果を確認」で照会する。初回登録には重複を防ぐ操作IDが無いため、再登録・再POSTをしない。照会できなければ「編集作業」の一覧や管理者に結果を確認する。未解決のまま別文書を作らない。

公開・予約公開の確定結果が不明なら、未公開や旧公開維持と断定せず、確認ダイアログの「同じ内容で再試行」で同じ操作ID・同じ対象・同じ要求を再送して結果を確認する。要求は公開画面の一時状態に保持されるため、版の変更、公開方法・予約日時の変更、公開画面の開き直し、画面からの離脱、ページの再読み込み、タブ終了を避ける。既に元の要求を失った場合は新しい公開要求を送らず、管理者に元の操作結果を確認する。公開成功を確認するまでは `seed-work` を実行しない。

次は**既存のLinuxサーバー側terminal**で行う。保存済み設定を読み、控えた公開済みDocument IDを入力してWorkの合成タスクを作る。

```bash
source "$HOME/knowledge-platform-poc/config/runtime.env"
read -r -p '公開済みの合成documentId: ' KP_ORGANIZATION_DOCUMENT_ID
export KP_ORGANIZATION_DOCUMENT_ID
KP_ORGANIZATION_PROFILE=sales-01 "$KP_SOURCE/target/debug/organization-server" seed-work
```

seedは既存Workをリセットせず、新規fixtureだけに完了/保留/再開を含む定義versionを使う。migration適用だけで既存workflowの定義・担当・進捗を変更しない。以前のforward-only/差戻/完了のみの定義や別の入力文書から作り直す場合は、このDBを上書きせず新しい専用環境で行う。予約公開schedulerはこのOrganization手順では起動しない。予約取消GUIがあっても、予約時刻の自動公開が稼働することを意味しない。[Document PoCのscheduler起動例](document-poc-runtime-v0.md#scheduler-and-other-boundaries)は `KP_RUNTIME_MODE=poc` 用であり、`organization-synthetic` のrequesterを解決しないため流用しない。

確認する操作:

- [ ] 営業が文案を保存し、事務には未提出本文が見えない
- [ ] 営業と事務のタスク内で公開改訂・内容の版・原本一覧を確認し、明示取得した原本を確認する。文書参照だけでTaskや未保存入力を変更しない
- [ ] 営業が提出、事務が引き受けて提出内容を読む
- [ ] 事務が理由を付けて差戻し、営業が新試行で修正・再提出する。旧提出は変わらない
- [ ] 根拠・候補・採用/修正/却下を作り、明示選択分だけ提出へ含める
- [ ] 選択した根拠を使って合成Agentを明示実行し、候補を人間が採用/修正/却下する。Agent結果だけで提出や完了が確定しない
- [ ] 営業/事務の担当中タスクを保留し、同じ試行・担当・private保存内容のまま再開する。未保存入力はタブ内だけで、自動保存しない
- [ ] 最終事務タスクを明示完了し、過去提出・根拠・判断・Agent結果を現在権限で読めること、新しい担当/提出が作られないことを確認する
- [ ] 両HTTPプロセスを正常停止して同じ設定で再起動し、完了状態・保存済み内容・操作結果と非公開分離を再確認する

文書のRoot直下作成・フォルダーの続き表示・選択親への子作成・改名・移動・属性3項目/未読/作成日時の絞り込み、文書の所属移動・正式改訂/比較結果/イベント履歴の続き表示・通常詳細からの旧版/原本確認、属性編集・既存複数原本の選択差替え・予約取消・公開状態の操作は[文書GUI手順](document-gui-v0.md)を参照する。初回登録は単原本で、複数原本の追加登録は今回含まない。

追加GUIの手動確認（所有者が今後行う項目。未準備・未実施は未確認と記録し、CI成功だけではチェックしない）:

- [ ] 現在の操作可否を読み取り、短い合成名と理由でSystem Root直下にフォルダーを作り、成功通知とIDを控える
- [ ] 読める非root親をツリーで選び、登録先の名前・IDを確認して合成の子を作り、成功結果と子IDを控える
- [ ] 続きがある場合は「さらに表示」で既表示行と選択を保持する。対象機で200件以下なら続きを未確認と記録し、201件表示の合格とは書かない
- [ ] 作成した子を同じIDのまま短い合成名へ改名し、一覧再読取で確認する。節7の正常停止と同じ設定でのHTTP再起動後も同ID・新名を確認する
- [ ] 文書種別・所管部署・カテゴリに短い合成値を持つ確認用文書で、3条件一致、1条件だけ不一致、属性解除、詳細往復と条件保持を確認する。絞り込みだけで元の属性・版を変更しない

- [ ] 読める別親を明示選択し、対象ID・現在親・移動先と継承アクセスへの影響を確認して合成の子を移動する。旧親から消え、移動先で同ID・同名を読めることと、節7のHTTP再起動後の現在親を確認する。実ACL変化やGUI通信断を確認したとは記録しない
- [ ] 公開一覧で「未読のみ」を明示適用し、詳細往復と解除で他の有効な条件が保持されることを確認する。閲覧だけで既読を記録したとは扱わない
- [ ] 作成日時の開始を含み終了を含まない条件、詳細往復・条件解除を合成文書で確認する。精密URLを使う場合は元の日時原文が保持されることを確認する
- [ ] 読める合成文書の詳細で「文書を移動」を開き、現在の文書・元所属・移動先と継承アクセスへの影響を確認して移動する。新しい読取で現在所属を確認し、節7のHTTP再起動後も同じ文書ID・所属を確認する。権限変化や通信断の確認を行ったとは記録しない
- [ ] 正式改訂が2件以上ある合成文書の「版・改訂」で履歴を読み、基準・対象を明示して「新旧比較」を開く。正式改訂を先頭から読み直した後と節7のHTTP再起動後にも同じ選択で読取・比較を確認する。対象機で100件以下なら101件目以降の実GUI読取は未確認と記録する

詳細は[既存の操作手順](organization-browser-poc.md)に従う。画像、ログ、DB、storageを外部へ送らず、結果だけを記録する。

## 7 正常停止と再開

両ブラウザーの操作・downloadを終了し、営業と事務の各terminalでCtrl+Cを1回送る。両方の `organization-server: graceful drain complete` とプロセス終了を確認する。処理中のstreamには全体の強制終了期限がないため、完了しない場合は接続中clientを確認する。強制killを正常停止と扱わない。停止/再起動時に残った未完了Agent実行は `outcome_unknown` として扱い、自動再実行しない。元の実行ID・operation IDで保存結果を確認する。

DBも停止する場合は、両アプリが終了してから行う。volumeは削除しない。

```bash
source "$HOME/knowledge-platform-poc/config/runtime.env"
docker stop --timeout 60 "$KP_DB_CONTAINER"
docker inspect --format '{{.State.Status}} exit={{.State.ExitCode}}' "$KP_DB_CONTAINER"
```

Dockerはtimeout後に強制終了し得る。正常終了でなければその事実を記録し、復旧確認前に安全としない。再開は `docker start "$KP_DB_CONTAINER"` 後、節3のreadiness確認、節5のアプリ起動、節6の保存済み状態確認を行う。初回の `migrate`・`bootstrap-poc`・`seed-work` は同じ版の通常再開では繰り返さない。

## 8 停止時バックアップ

**両アプリと、このDB/storageへ書く他の全プロセスを停止した状態で、DBとstorageを一組として保存する。DB自体は起動したまま。** `pg_dump` 単体の整合性は、別filesystemとの整合性を保証しない。

```bash
source "$HOME/knowledge-platform-poc/config/runtime.env"
set -euo pipefail
set +x
umask 077
BACKUP="$KP_HOME/backups/$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -m 700 "$BACKUP"
docker exec "$KP_DB_CONTAINER" pg_dump -U postgres -d "$KP_DB_NAME" --format=custom \
  > "$BACKUP/database.dump"
tar -czf "$BACKUP/storage.tar.gz" -C "$KP_STORAGE_ROOT" .
printf '%s\n' "$KP_SOURCE_SHA" > "$BACKUP/source.sha"
cp "$KP_HOME/config/db-image.txt" "$BACKUP/db-image.txt"
cp "$KP_HOME/config/runtime.env" "$BACKUP/runtime.env"
cp "$KP_HOME/config/postgres.env" "$BACKUP/postgres.env"
(cd "$BACKUP" && sha256sum database.dump storage.tar.gz > SHA256SUMS)
test -s "$BACKUP/database.dump"
```

backup内の設定も秘密情報を含む。source/binary/GUI/PDFiumを含む元のreleaseディレクトリも保持する。これはローカルPoCの停止時保存例で、暗号化した別媒体保存、保持期間、定期実行、災害復旧、復旧時間の資格取得は別途必要。[pg_dumpの範囲](https://www.postgresql.org/docs/18/app-pgdump.html)

## 9 元環境を壊さない復元確認

所有者自身が作成したbackupだけを使う。元DBと元storageは残す。両アプリを停止したまま、backupディレクトリを選び、hashを検証する。

```bash
source "$HOME/knowledge-platform-poc/config/runtime.env"
set -euo pipefail
set +x
umask 077
read -r -p '復元するbackupディレクトリの絶対path: ' BACKUP
test -d "$BACKUP"
(cd "$BACKUP" && sha256sum -c SHA256SUMS)
test "$(cat "$BACKUP/source.sha")" = "$KP_SOURCE_SHA"
KP_RESTORE_DB="kp_org_restore_$(date -u +%Y%m%d%H%M%S)"
KP_RESTORE_STORAGE="$KP_HOME/storage-$KP_RESTORE_DB"
mkdir -m 700 "$KP_RESTORE_STORAGE"
docker exec "$KP_DB_CONTAINER" createdb -U postgres --template=template0 "$KP_RESTORE_DB"
docker exec -i "$KP_DB_CONTAINER" pg_restore -U postgres -d "$KP_RESTORE_DB" \
  --exit-on-error --single-transaction < "$BACKUP/database.dump"
tar -xzf "$BACKUP/storage.tar.gz" --no-same-owner -C "$KP_RESTORE_STORAGE"
export KP_DATABASE_URL="${KP_DATABASE_URL%/*}/$KP_RESTORE_DB"
export KP_STORAGE_ROOT="$KP_RESTORE_STORAGE"
for key in KP_HOME KP_SOURCE_SHA KP_SOURCE KP_DB_CONTAINER KP_DB_VOLUME KP_DB_NAME \
  KP_DATABASE_URL KP_RUNTIME_MODE KP_STORAGE_ROOT KP_DSI_WORKER KP_DIFF_WORKER \
  KP_WEB_DIST KP_DSI_PDFIUM_RUNTIME_DIR; do
  if [[ "$key" == KP_DB_NAME ]]; then
    printf 'export KP_DB_NAME=%q\n' "$KP_RESTORE_DB"
  else
    printf 'export %s=%q\n' "$key" "${!key}"
  fi
done > "$KP_HOME/config/restore.env"
chmod 600 "$KP_HOME/config/restore.env"
```

復元先は空DBなので、先にmigration/bootstrap/seedを走らせない。`pg_restore --single-transaction` は復元SQLを一括transactionで処理する。失敗時に `--clean`、ledger修正、元DB削除で続行しない。[pg_restore](https://www.postgresql.org/docs/18/app-pgrestore.html)

節5の2つのterminalで、読み込むファイルだけを `config/restore.env` に変えて起動する。health、合成文書の原本、提出・差戻・根拠/判断・合成Agent結果・完了/保留状態・非公開分離、以前の保存状態を確認する。元環境と同じportなので同時起動しない。元環境へ戻る場合は復元側を正常停止し、元の `runtime.env` で再開する。復元コピーへの新しい書込は元DBへ戻らない。

## 10 更新と切戻し

1. 新しい受入済みcommit SHAとそのexact CI結果を決め、別のreleaseディレクトリへ取得・ビルドする。稼働中のcheckoutやbinaryを上書きしない
2. 新旧のmigrationファイル・台帳・環境変数・操作仕様を比較する。新headに本書の固定SHAだけを差し替えて実行しない
3. 両アプリを停止し、節8のDB/storage/設定/releaseを保存する。節9の**別DB・別storage**で新しい候補のmigrationと起動・業務・復旧を先に確認する
4. 検証できた変更だけを所有者が適用する。更新対象の実DBに対するmigrationは明示操作であり、Gitのmergeでは実行されない
5. schema/dataに変更がないことを確認できる場合のみ旧releaseへの切替を検討する。旧binaryがschema不一致で拒否したら、保護を解除しない
6. schema/data変更後の切戻しは、互換性を確認したforward fix、または更新前のDBとstorageをセットで別環境へ復元して旧releaseを起動する。更新後の書込を失う可能性を所有者が判断する

**Git revertはDB migration、提出済みデータ、原本storage、外部へ送った情報を戻さない。** 下りmigration、DB巻戻し、旧ledgerへ偽装するコマンドは提供していない。

現在の統合注意点:

- この固定版ではDocument `0009_document_revisions_v0.sql` / `0010_document_version_updated_at.sql` を保持し、OutboxをSQL本文不変で `0011_outbox_delivery_v0.sql` へ配置済み。旧Search `0009_outbox_delivery_v0.sql` 適用済み・不明履歴は変換せず停止する。[判断記録](../decisions/2026-10-04-search-main-migration-integration.md)と[STOP条件](search-main-migration-stop.md)に従い、既存DBへこの初回手順を流用しない。所有者の実環境に旧Search9がないことは未証明
- Work 0001〜0006はDocumentと別の `work.schema_migrations` 台帳を使う。旧checksumは保持する。合成Agent/完了/保留再開は新規fixtureの定義を使用し、既存workflowの定義を自動昇格しない。モデル用秘密情報や新しい認証設定は不要
- 旧PR82のpersistence失敗とPR83のOrganization HTTP503は原因未特定の履歴として保持する。後続headの合格だけで原因を修正済みとしない。PR87の同Root継承・文書なしのフォルダー移動fixtureと、PR88の同権限の文書移動fixtureを、実ACL変化やGUI通信断の資格へ広げない
- PR89の過去4runの実受入失敗、有限診断の解釈訂正、別のHome focus競合残件は[正式改訂の記録](../superpowers/execution/document-revision-pagination-status.md)に保持する。現在mainの合格だけで旧失敗原因をすべて解消したとは扱わない
- PR62初回の再起動後read失敗とPR65初回のresponse.body()観測bytesの実encoding原因は未特定。PR67の実Download照合・再起動後復元成功を、対象PCの復旧資格や原因解消と読み替えない

## 11 本番利用開始までの未達項目

- [ ] OSのdistribution/version、運用ユーザー、RAM/ディスク容量、backup先、接続方式を決定し、対象機で既存worker保護を実証する
- [ ] production Identity adapterを実装・受入し、実利用者とrole/assignmentを現在の認可に結び付ける。固定profileを本番認証として使わない
- [ ] TLS、公開範囲、DNS、reverse proxy、DB最小権限、secret管理、service定義、起動順、監視・アラートを設計して受入する
- [ ] 対象実DBのmigration履歴・互換性を確認し、Document/Organization/Searchを組み合わせる対象commitで統合試験を行う
- [ ] Audit配送・保存、Search全体、Agent連携など必要機能の未完gateを閉じる。個別schemaや合成PoCの合格で代替しない
- [ ] 定期backup、暗号化、別媒体保存、restore演習、切戻し時のデータ損失/RPO/RTOを決めて検証する
- [ ] 本番の対象releaseと利用範囲を確定する。現在のmain mergeはCIを起動するが、production配備は行わない

## 根拠と保守

- [Organization設定と固定profile](../../crates/organization-server/src/config.rs)、[実装済みCLI](../../crates/organization-server/src/main.rs)
- [Document runtimeの境界](document-poc-runtime-v0.md)、[文書GUIの登録・属性編集・原本差替え・公開操作](document-gui-v0.md)、[Organization操作](organization-browser-poc.md)
- [固定toolchain](../../mise.toml)、[Work migrationと台帳](../../crates/work-repository-postgres/src/lib.rs)
- [手順書の検証状況](../superpowers/execution/linux-manual-installation-guide-status.md)

OS/版の選定後にOS固有の準備手順を補う。新しいソース・migration・認証方式を採用した場合は、本書の対象SHAと受入記録を一緒に更新する。

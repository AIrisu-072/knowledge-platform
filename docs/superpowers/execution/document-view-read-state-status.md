# 文書詳細表示による既読・未読戻し：再実装の状況

Status: ACTIVE

## 2026-10-07 23:11 UTC — 手順headの全CI合格と最新mainの追従

- 公開head e08365562987b098e175904688c37035b311f940 / tree0788260d407d364910279429b6aeaab5a323e035の[CI37698468205](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37698468205)は終端SUCCESS。14 jobs全成功、19checks16成功・既存skip3、artifact0。Document/Organization/summaryの3step成功。Rust実ログ113055908096で1980成功/13skip＋21成功/0skip＋7成功/1skipを確認した。checkoutはmerge ref26fe02401a674412d6ac67cf27552cf7e6557118で、headと同tree、parentsは旧main a7cf93d53とe0836556である
- 導入4手順のsource/日本語レビューGO、旧pin/main資格の残存表記と履歴日時labelを補修して同PRへ保存済み。製品cbe65d14へのpinは保持する。大runtime stdoutは個別未読で、公式stepと固定sourceの失敗伝播による評価・未資格の境界は変わらない
- CI確認中に別担当のAudit Infrastructure単位A/PR98がmain643cc85d47b5bcc48ad7b6f19f05fa2f653902a1へ統合されていたことが分かり、PR106がdirtyとなった。新mainは33fileで、重なるのはactive.md先頭の追記のみ。Audit側32fileは新mainのbytesを保持し、activeは双方の節を削除せず併記する。Document/GUI/既存受入/導入手順の追加変更はしない
- 次のexact action: 同じPR106へmain追従のmerge commitを保存し、新しい組合せheadの通常CIを終端まで確認する。旧baseでの合格を新組合せへ転用しない。mergeは親担当、実server反映は依頼者の手動操作

## 2026-10-07 22:38 UTC — 製品headの通常CI合格と導入手順の更新

- 公開head `cbe65d140852cbacd7fea4a8fed7830f0757ea4b` / tree `b2cb1cba60bfc29c4407335faec8f30a6d1c9740` / base `a7cf93d53a1b1627ace31d079fd222d7400d8673` をGitHubから再確認した。[CI37692284389](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37692284389)は終端SUCCESS。通常14 jobsすべて成功、全19 checksは16成功・既存skip3、公開artifact0件。mainは同baseを保持している
- [Rust実ログ113035189328](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37692284389/job/113035189328)はmerge ref `1d3a4ab197935122cc7d61e804c3ccecff30e502` をcheckoutしている。そのtreeは公開headと完全一致し、parentsは同mainと公開headである。新CurrentRead純粋6件・HTTP純粋4件・実DB15件・移行1件・HTTP reset/replay・旧PUT実DB7件のPASSを直接確認。workspace1980成功/既存skip13、追加21成功/skip0、別7成功/既存skip1。Rust staticのfmt/check/clippy/sqlx、container、required-checkも成功
- [Document runtime113035189564](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37692284389/job/113035189564)の通常head checkout指定sourceで、Real composition-root acceptance・Real Organization two-principal acceptance・Emit bounded runtime evidenceの3stepが成功した。固定runner/summaryの全必須工程、Agent provenance、同じowned DB/storageによるHTTP再起動、本人の未読snapshot/古いVIEW replay、正常終了/owned cleanupの失敗伝播と成功stepを照合し合格と評価する。大ログはTransport closedで個別実測値を直接読んだとは扱わない。画像/macOS golden、DBプロセス再起動、対象PC/本番Identity/TLS/backup-restoreの資格は追加しない
- local HEAD `0040f7a15017584c7543379df3d98a35558bf07f` は公開commit IDと異なるが同treeで、開始時worktreeはcleanだった。保存済みsourceを復旧元とし、旧未公開sourceや以前の成功を転用していない
- 導入4手順を資格済み製品head cbe65d14へ固定する。Document12/Work9、work-artifactsを含むstorage全体、既存Workのmigrate→同じDocumentIDでseed-work→serveをsourceと照合する。最小2profileを維持し、追加6profile常設を導入条件にしない。新機能sourceは変更しない
- 導入4手順をsourceへ照合し、shell構文24ブロック・相対file link96件・固定source link7件・旧migration17file不変・git diff checkを確認した。既存runner/summary/metadataの純粋source guard25件も固定Node24.21.0で成功し、repo:policyも成功。独立レビューGO。残った旧pin/main資格の文言を修正し、初回記録日時のlabelも画面へ揃えた。手順の実コマンド/対象PC/backup-restoreを実行済みとは扱わない
- 次のexact action: 同PR106へ手順/状況を保存し、保存後exact-headの通常CI全終端を確認する。PRはDraftのまま。最終headのmerge-ready判定と親によるmain統合・統合後CIは未完、実server反映は依頼者の手動操作

## 2026-10-07 21:44 UTC — 新版作成の成功通知を確認する受入の限定修正

- 公開head2cd9c75b / tree ab3552e2のCI37635679777で、本番振分け37case、新DB15件・移行・HTTP・旧PUTを含むRust1980件は成功。全体はDocument runtimeとrequired-checkが失敗し、14 jobs中12成功・2失敗、公開artifactは0件だった
- 依頼者が共有した同headのbounded summaryでは画面12成功・1失敗・5skip。前回のmetadataはgui-metadata-snapshot-savedまで成功し、残る失敗はdocument-runtime.spec.ts:473:93のtoContainText、完了点はversion-form-opened。473行は新版POSTの201確認直後の成功通知assertionである。後続Agent・HTTP再起動は未実施
- 今回追加した受入のlocatorは「新版作成」region内のstatusを要求していたが、実際の成功通知は兄弟の「作業版の編集」regionにある。旧「新版作成」regionは空で、実route DOM試験でも成功文言自体が存在する一方、そのregion内ではstatusが見つからない同じREDを再現した
- 製品sourceは変更せず、受入のregion名1箇所を実作業文脈へ修正する。既存DOM試験はauthoring新版作成と公開時のそれぞれのregion内に成功statusがあることを確認し、全体statusへの緩和はしない。公開側の「公開・予約公開」scopeは既存DOMと一致している
- ローカルで同DOM試験54件、全GUI73suite/1722件、runtime型と18件の試験収集が成功。追加assertionのTesting Library型ではexact optionが未対応だったため、同じ完全一致の正規表現へ直し、最終54件とGUI型を再確認した。独立source/RED/GREENレビューはGO。新規試験基盤、timeout、skip、期待201・成功文言は変更しない
- API/consoleの失敗8件は直前情報がなく、このlocator不整合の根因には使わない。apiEventsは先頭12件だけである。修正後のhosted画面・再起動・cleanup合格はまだ未取得。導入pin、画像資格、別の開発StrictMode反例の制限は保持する

## 2026-10-07 14:14 UTC — 本番API振分けの実REDと限定補修

- test-only公開head8fd2f57d / tree d0d7ec9cのCI37632618755で、新GET read-stateが実際にread familyへ送られるREDを確認した。Rust job112830522356、contract.rs:182:9の期待management/実readであり、compile失敗ではない。最初のGETで停止したため、後続POST2操作の個別REDを見たとは扱わない
- その後、本番dispatcherの既存read-state判定へGETを加え、POST read-state/view・resetを同じmanagement familyへ送る数行だけを補修した。各handlerの最終認可・型・body・OCC・業務意味、旧PUTと他route familyの順序は変更しない
- 最新の製品差分はapi.rsだけ。既存合成契約の37caseと実runtimeを次の同PR通常hostedで確認する。ローカルRustを実行したとは記録せず、新headのGREEN/全画面/再起動はまだ未取得。以前の失敗・ユーザー共有summary・別の開発StrictMode反例は保持する

## 2026-10-07 13:51 UTC — 実受入の共有診断と本番API振分けの欠落

- 第5公開head671fb237 / tree965ec049のCI37616879956は終端FAIL。Rust1980件、新DB遷移15件・移行・HTTP・fmt/clippy/sqlx等は成功したが、Document実画面は11成功/2失敗/5skipで、後続Agent/再起動は未実施。元の大ログはTransport closedのため、依頼者からbounded summaryの共有を受けて再開した
- 失敗のdocument-runtime.spec.ts:25とmetadata-editor.spec.ts:159はtest宣言であり、assertionの場所ではない。完了点はそれぞれdocument-selected、gui-metadata-working-verified。ブラウザーAPIの失敗0はNode側のSDK呼出しを含まず、apiEventsは最初の12件であるため失敗直前の応答と解釈しない
- 新GET read-state、POST read-state/view・resetはmanagement routerにあるが、本番compose_document_apiの振分けは旧PUTだけを登録しており、新3操作はread routerへ落ちることを独立に照合した。新HTTP試験はmanagement_router.merge(read_router)を直接使い、本番振分けを通らなかった。既存の合成契約にも新3操作が不足していた
- 画面遷移前に新GETを呼ぶ両試験の位置と、この欠落は整合する。実SDKへ空本文404を返す限定試験では{}がthrowされ、画面遷移0・既存診断のerrorCategory unavailableを再現した。実失敗HTTP本文そのものを取得したとは主張しない
- 本番相当の非同期DOM対照は成功。lazy routeと開発時StrictModeだけの別反例は証拠として保持し、今回の本番受入原因や合格へ混ぜない
- まず既存Rust合成契約へ3操作を追加し、34→37caseへ拡張する。local Rustは未使用で、この試験sourceのREDを同じDraftの通常hostedで確認してから、振分けだけの最小補修を重ねる。新runner・timeout/skip緩和・新機能は追加しない。導入pinは合格前に変更しない

## 2026-10-07 11:47 UTC — 最終ローカルGUI確認とhosted追従補修

- 第4公開head0b3987ad69d74378b2203a8d526ab2ab5b2ba931 / tree8b5aeb2449cc844810b86046642a4452f69ddef4は66ファイルが一致し、CI37615369766を開始した。旧75件境界の全GUI73suite/1721件・build成功を確認したが、独立レビューで回復panelからの再取得中に待機中VIEWを送る反例1件を発見した
- その反例を新しい実route REDで再現し、共有refreshの同期opening失効とVIEW送信直前のlive query/owner資格確認へ限定補修した。取得中も完了後も未読を維持する反例がGREEN。初回記録日時の履歴label/assertion各1箇所も揃え、focused124件・全GUI73suite/1722件・schema/型/buildが成功。独立した同反例の再検査を含む仕様/品質再レビューはGO
- 第3headの通常Document GET422は、新HTTP試験だけが既存必須query viewを欠いたことをread.rs/OpenAPI/既存成功例で確認した。試験URIへview=publishedを追加し、失敗時に合成response bodyを示す2行だけ補修する。業務判定や期待200は変更しない
- 第4headの実fmt logが示した残8hunkを同sourceへ一致確認して適用する。既存Search回帰は同headで成功し、台帳期待1箇所の追従を確認した。新Rust/DB/HTTP全体、追加GUI受入・再起動の最終合格は次headで確認する。過去の失敗/未取得と未資格は保持し、同PRへこの限定補修を保存する

## 2026-10-07 11:28 UTC — GUIと既存受入の段階保存

- GUI所管15filesを新focused75件・schema/型の成功時と同じhashで固定した。StrictMode/remount/reload、遅い読取/新版/認可拒否、MAX、UNKNOWN往復/固定再送、成功後read失敗を含む。補助read拒否後の正規再確認が余分にretryする実反例は、既存queryFnを明示再利用して1回へ限定した。全GUI/buildとGUI独立レビューは続行中
- 既存runtime4filesは新しい型検証・純粋/source guard43件・MCP compile・試験収集18＋5が成功し、sourceの独立仕様/品質レビューはGO。これは実DB/browserの合格ではない。metadataの再入場前未読snapshot、regulationの最後の閲覧後RESET、GUI表示前の再起動後replayを保持する
- backendのreceipt schemaとRESET後VIEWの指摘を補修。実fmt logの一致分だけを適用し、不一致6hunkは残して次のexact-head hostedで確認する。台帳最大番号追従は1assertだけ。API20件・SDK12件の新しい成功とRust/DB未資格を区別する
- 第3公開head a626b54bの既存Document/Organization runtime jobは成功したが、新GUI/受入sourceは未収録なので今回の既読方式の資格にはしない。旧1ebのOrganization失敗ログは同じtoolの限定再確認でもTransport closedで原因未特定のまま保持する
- この保存では操作/移行の日本語追補も同梱する。現固定導入SHAは保持し、未資格の候補を導入済み/導入可能とは表示しない。次は新しい全GUI/build/独立レビューと同PRの通常hostedを確認する

## 2026-10-07 11:20 UTC — 第3保存と独立レビュー補修

- PR106の公開head a626b54b4aafcaef7a78978e60ea19004036e6d7 / tree cccd384eaded8d941b4c09d3370f006de5042b64 / base a7cf93d53をreadbackで照合。43ファイルを同じDraftへ保存し、本文も一致した
- 中間head1eb06cafのCI37610285070は失敗。Rust compile後の新純粋6caseはPASS行を確認したが、全体はDocument台帳最大番号の期待11/実12で停止した（outbox_delivery_migration.rs）。既存Search回帰も同じassertionが原因であり、Document migration追加に必要な1箇所だけ12へ追従する。新しいSearch機能/専用実行は追加しない
- 中間headのfmt実diffをsource一致確認して反映する。Organization実受入stepも失敗し、元の接続済みjob-logのTransport closedで原因未取得。失敗や未取得を後続headの資格へ付け替えない
- 新backendの独立レビューは成功receipt schemaのnull日時/revision0許容、RESET r2後VIEW r3の成功試験不足の2点でNO-GO。前者は実schema反例の新REDから限定補修し、API20件/SDK12件・生成/型/lintが成功した。後者とformatter/台帳は追加sourceで次のhosted確認へ進む。再レビュー前でありbackend全体の合格ではない
- 新GUIは実routeの正しい通常表示を先に確認した欠如REDから、核37件＋先行実route8件/計45件とschema/型が成功した小単位を保存済み。広い反例・全GUI/build・独立レビューは続行中。旧sourceの結果を転用しない
- 既存4受入fileを新sourceで再構成中。日本語の操作/移行互換手順を同機能へ追加し、旧固定導入版に今回の既読方式がないことを明記する。実DB/画面/HTTP再起動の新資格とmain統合はまだ完了していない

## 2026-10-07 10:58 UTC — backend試験・規範の保存

- 第2公開head1eb06caf/tree324ad1bbのcoreに、新しい純粋HTTP4case、DB transaction15case、migration反例、既存projection/legacy/HTTP受入のassertion、規範追補を加える。API全38操作のschema/evidence登録も新sourceへ揃える
- 新NodeのAPI19件・SDK12件、生成/型/lint/diff checkは成功。Rust本体・追加試験はローカルfmt/compile/test/clippy未実施であり、次の通常hostedで確認する。旧sourceの合格を転用しない
- 初段のNode/Rust欠如RED、別のfmt失敗、作業環境の喪失は記録を保持する。backend sourceは独立レビューへ渡し、GUIの新sourceは別の小単位で同じPRへ保存する

## 2026-10-07 10:40 UTC — 第2保存単位のbackend core

- PR106の初段head5fca7835/tree0d592f03を公開し、6fileのblob・tree・parent・branch・本文を照合した。同head CI37606340153の実ログで新GET欠如のNode REDと、current_read_state_contract.rsの新契約未定義E0432を確認した。fmt失敗も別に記録し、hostedの実formatter diffに従って修正した
- 新backend core25filesはAPI/SDK生成、API契約19件・SDK試験12件・型/lintが成功。Application/共有projection/HTTP/migration0012/receipt・CAS・Auditのsourceを保存する。旧PUTと既存wire形を保持する
- この段階のRust sourceは新しいcompile/実行が未完。DB/HTTP反例sourceと関連規範は続行中であり、backend完成・実DB合格とは扱わない。GUIも新しい欠如/可視性/navigation反例から再実装中
- 次は同じPRへ残る試験とGUIを追加し、通常hostedのRust/実DB/実GUI資格を確認する。初段のbaseline runtime成功は今回の新機能成功へ転用しない

## 2026-10-07 10:02 UTC

- 利用者が承認した詳細正常表示→既読、未読戻し→再確認目印、初回日時/過去Audit保持、読了証明に使わない意味を再実装する。[限定要件](../specs/2026-10-07-document-view-read-state-design.md)と[計画](../plans/2026-10-07-document-view-read-state.md)に従う
- 先の未公開sourceは実行環境の接続障害後に取得できず、残存bundle/patch/保存物にも今回の全文を確認できなかった。旧sourceの復元や旧成功件数の継承とは扱わず、公開main a7cf93d53a1b1627ace31d079fd222d7400d8673 / tree8f2834fbc9f46f169a5069778ec56b191ba9e09eから新しく試験を行う
- Document migration0012は未使用で、既存台帳期待は1..=11。既知のquery reset同期描画・同tick Reset・CSS非表示祖先と、受入の再入場前未読snapshot確認を先行する
- Node24.21.0/pnpm12.4.1は固定lock/公式供給元検査を通して依存導入済み。初回tarballの接続失敗を保持し、同一手順の通常再試行で完了した
- Rust1.98.1の公式manifest/checksumは取得できたが、記載のcomponent archiveがHTTP403。追加経路で取得を迂回せず、ローカルRust compile/純粋試験は未実施とする。新しいRust試験は同じDraftの通常hosted CIで確認する
- 新Node API契約は既存15件成功・新契約1件失敗となり、新GET operationIdが存在しない実AssertionでREDを確認した。新Rust純粋契約6caseのsourceも追加したが、compile/実行はまだしていない。この小単位を保存し、同じDraftへ段階的に実装する。機能・GUI・受入は未完成。CIの期待するRED、実失敗、未実施を区別し、最後に新sourceを資格化する。main mergeは親担当、実server反映は利用者の手動操作

ローカルDB/socket/Docker/Chromium、新runner/画像/timeout/skip緩和は行わない。他担当のTauri/Org/Audit配送/Search製品を保持する。通常hostedの実DB/GUI/HTTP再起動/cleanupはまだ未実行である。

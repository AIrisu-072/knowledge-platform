# Document 段階的負荷検証

既存 Common Document API の実プロセスへ、公式 PDF を少量から投入する検証用ツールです。このツール自体はSearch、GUI、scheduler、権限の意味を変更しません。同PRの承認済みPDF意味検査拡張は、専用設計と回帰試験で管理します。本番運用資格や性能 SLO を宣言するものではありません。

## 実行したものと、これから実行するもの

`test/*.test.mjs` は Node の契約・安全性試験です。fake transport / injected runtime の成功を実 API・PDF 公開・大量件数の合格に使いません。契約試験の report は `evidenceClass: test-double` となり、後続の大量段階へ進む根拠にはできません。実行記録は専用の [状況](../../docs/superpowers/execution/document-load-qualification-status.md) を参照してください。

## 所有範囲と入力

- `tools/document-load-qualification/`：生成 SDK と BinaryTransportBridge を使う client、operation journal、容量 admission、計測、来歴、試験
- `tools/document-poc-runtime/run.mjs`：既存の実プロセス受入が完了した後だけ動く optional hook
- `.github/workflows/ci.yml`：既存 PR label イベントの `document-load-small` に限る small opt-in。権限・runner・job 数・timeout は不変
- このハーネスは既存 seed/API/権限を迂回せず、Search corpus branch/PR85、active pointer、導入手順は変更しない

小量入力は厚生労働省の公式通知PDFの正例2件（001472934、000616197）と負例1件（001472933）です。正例は横書きの実通知で、負例の縦書き・文字対応表のないfont構造は現在の限定対応外です。URL、発出日、取得日時、PDL1.0 利用条件、バイト数、SHA-256 は `official-sources.json` に固定しています。元ページは https://www.mhlw.go.jp/stf/newpage_56768.html 、利用条件は https://www.mhlw.go.jp/chosakuken/ です。出典：厚生労働省。本文に第三者素材がある資料を追加する場合は、別の権利確認が必要です。

原本 bytes は repository に保存しません。実行時に同じ公式 URL から再取得し、サイズ・SHA-256 が異なれば停止します。出典・URL・発出日・license は検証来歴に残し、文書の索引用 metadata には入れません。多数件は同じ 2 原本を繰り返す **合成の容量負荷** であり、ユニーク 1,000/1万/10万文書の検索精度試験ではありません。2 件の異なる通知を同一テスト文書の第 1・第 2 版に使いますが、実際の法令・通知の改正関係を意味しません。

## 契約試験

repository の固定 toolchain と frozen dependencies を用意してから実行します。

```sh
pnpm --dir tools/document-load-qualification test
```

追加 dependency はありません。ローカルの Node 版が pin と異なる場合は、その差を記録し、hosted の実受入へ読み替えません。

## 小量の実受入

既存 runtime の Linux sandbox/PDFium/build/owned DB/storage 前提をそのまま使います。既存手順で `mise run document:poc:agent` が実行できる環境に限ります。

```sh
KP_DOCUMENT_LOAD_SMALL=true mise run document:poc:agent
```

CI では専用検証 PR に `document-load-small` label を付けると、既存の `pull_request: labeled` から通常 CI を実行します。該当 Document job だけ small mode が有効になります。branch/ref/head と run URL を記録してください。label のない PR と main は従来通りです。label は全通常 CI を開始するため、既存の通常 CI 消費はあります。新たな外部資源、有料 API、credentials、permission、artifact の包括 upload は追加していません。

small mode は公開対象2件と独立した拒否対象1件、逐次要求、測定対象 5 分以内、空き disk 1 GiB、空き RAM 512 MiB、測定対象 process-tree RSS 2 GiB を試験の停止線にしています。これはハーネスが選んだ小量実行の保護値であり、利用者の容量上限や製品 SLO ではありません。`--prebuilt` や外部 DB 指定との併用は拒否します。

## 何を確かめるか

1. process-fixed `poc-human` / `poc-agent` と既存 root policy を確認。別のhuman-only folderへ負例を登録し、公開422/BUSINESS_RULE_REJECTED・DSI不在・同じ必須sandboxでunsupported_semantic_construct・保存原本の完全一致を確認
2. 自分のテスト Folder にだけ登録・公開。全ページ一覧を比較し、欠落・重複・cursor loop を拒否
3. 第 1 文書の metadata を更新して再読取し、古い revision での更新が 409 になることを確認
4. 異なる原本で WORKING 版を作成。公開までは current pointer が変わらず、公開後は新版へ切り替わることを確認
5. 既に Agent が読めた末尾文書を human-only に変更。既知IDが404かつProblemのstatus=404/code=DOCUMENT_NOT_FOUNDになることと、一覧からの除外を確認（この読取routeの存在秘匿契約）
6. 原本 SHA-256、detail、全 revision detail、version metadata、policy を保存
7. 既存 runner が human/agent 実プロセスを停止・再起動。異なる PID と、同じ DB/container/storage/run/head の来歴を確認
8. 同じ全一覧・拒否結果・snapshotを再照合。負例も作業版のまま、公開版なし、元hash・policy・DSI不在が保持されることを確認

small は全文書を詳細照合します。大量段階は登録・公開・一覧件数を全件、詳細・原本・履歴・再起動 snapshot は先頭・中間・末尾 3 件です。このサンプリングを「全件の原本検証」と呼びません。GUI/Agent MCP/その他の既存否定系は元 runtime が別に検証します。今回の新しい試験で Search 精度・検索反映・backup/restore・DB プロセス自体の再起動は検証しません。

## 次の段階

1,000 → 10,000 → 100,000 を一段ずつ実行します。自動連続実行はしません。CI の label では small 以外を指定できません。

実行前に private local JSON を作り、`stage` と `documentCount` を同じ値、`safetyFactor >= 1`、`budgets` の各数値、実行期限 `deadlineAt`、直前の成功 `report.json` の絶対パス `previousReport` を明示します。budget は `diskReserveBytes`、`minAvailableMemoryBytes`、`maxRssBytes`、`maxWallTimeMs` です。JSON に秘密値や endpoint は不要です。

```sh
KP_DOCUMENT_LOAD_PLAN=/absolute/private/stage-plan.json mise run document:poc:agent
```

直前までのすべての report が、同じ corpus/code/runtime fingerprint、実プロセス来歴、再起動証拠、全件の登録 ID、実測 metric を持つことが必要です。小量の違う code の成功を新 code の容量根拠に使いません。

- 時間・disk は前段の実測に件数倍率と安全係数を掛けて screening
- RSS は逐次実行の前段 peak × 安全係数。固定 server RSS を文書数で比例拡大しない。将来の memory 増加を保証するモデルではない
- 実行中は約 1 秒ごとと各 mutation 前に資源を確認。進行中 HTTP も budget abort signal で停止し、結果不明は journal に保持
- 観測できない値、余裕不足、前段不成功は `NOT_ADMITTED`。停止は `ABORTED`。失敗は `FAILED`。いずれも未実行・失敗を成功にしない

既存 owned PostgreSQL は tmpfs を使います。large admission では DB tmpfs と原本保存の filesystem を独立に測定し、空き memory にも注意します。100,000 を実行するために停止線を下げたり、未資格の保存構成へ自動変更してはいけません。必要なら、実測を報告して別の適切な配置を決めるところで止めます。`NOT_ADMITTED` は全依頼の完了ではありません。

## 計測と保存

実行ディレクトリの `document-load-qualification/report.json` に plan、前段 chain、状態、来歴 fingerprint、raw resource observations、各操作の件数/HTTP status/p50/p95/p99、全 stage 経過・処理量、sampled peak RSS、DB/原本論理増分、filesystem 割当増分を残します。

RSS は harness Node 自身、human/agent、worker 子プロセス、PostgreSQL の process tree を含みます。sample 間の一瞬の peak は捕捉できないことを記録します。diskGrowth は各 filesystem の peak 空き容量減少を個別に求め、論理増分を下限として合計します。同一 filesystem の場合は保守的に二重計上されます。ほかの process による disk 消費も含み得ます。これは stage 全体の観測で、純粋な server CPU/IO の内訳や本番性能保証ではありません。

原本・詳細・operation journal を含む private directory 全体を upload しないでください。CI stdout は固定状態と数値集計だけです。通常 runtime の最終 cleanup も確認してから結果を利用してください。

## 中断時

mutation 前に request を journal へ append/fsync します。operationId のある要求は同じ request だけ再生可能ですが、stage 全体の無条件 resume は提供しません。初回 create の response が不明なら同じ POST を再送しません。journal の破損・不完全な末尾・identity drift・別 writer は fail closed です。自動 DB reset/drop や原本削除は行いません。元 runtime の自分の container/process cleanup は維持します。証拠を保存して原因を調査し、必要な disposable reset を個別に判断してください。

### Search 投入器との関係

PR108 の運用追補にある「並行登録器を作らない」は、Search corpus の同じ DB/manifest へ別経路で重複投入しないための境界として維持します。読取確認した Search `ingest.py` は `text/plain` 固定で、別の未統合 branch の所有物です。今回の client は公式 PDF を既存生成 SDK で測る独立した Document API 受入用であり、Search corpus の取得・変換・投入・索引を置換しません。Search に接続する場合は担当と manifest/DB/source の所有を別途合意し、いずれか一方だけで投入します。

## 公開拒否の限定診断

失敗時は既存 Error Registry にある code と実 HTTP status が一致する場合だけ Problem code を記録します。detail、traceId、field error、本文は保存・出力しません。目標件数とは別に、journal の成功応答を保持できた登録数・初回公開数を confirmedCreatedDocuments / confirmedPublishedDocuments として示します。応答不明の場合は実際に保存された件数がこれより多い可能性があり、未保存を断定しません。途中失敗の数値は `partial-failed-stage` とし、容量・性能の成功根拠にしません。

公開拒否時に限り、自分の journal の pending publication に対応する FileId 1 件を、自分の disposable DB で read-only 集計します。DSI保存行の有無、PDF判定、原本 hash/size一致、未解決変更・embedded comments・invalid/unverifiable署名の件数だけです。これは Common API を迂回する製品読取機能ではなく、失敗解析用の検証計測です。コメント内容、作者、locator、署名主体、parser message はSQLでも選びません。未取得は unavailable / not-found のままで、拒否を成功へ変えません。

DSI行が無い `BUSINESS_RULE_REJECTED` の場合は、検査APIの呼び忘れと断定しません。現在のHTTP契約では、workerの未対応構造・文字抽出失敗等も同じ422へまとめられます。専用Cargo example `document-load-inspection` は、同runでbuild/test/hashを確認した既存LinuxSandboxRunnerへ、変更していない公式原本を渡す診断driverです。既存worker/PDFium、mandatory sandbox、既定10秒上限は維持します。直接worker起動、portable parserへのfallback、失敗時の許可はありません。

実行前に、自分のpending publicationのFileIdについて、保存済みhash・size・mediaTypeと公式原本が一致し、WORKING・未分類でない・単一authoritative参照であることをread-only確認します。一致しなければdriverも実行しません。診断は固定worker failure codeまたは件数/原本bindingのみを出し、常に `qualification:false` です。元のAPI公開FAILは保持します。この補助検査成功だけでは、API公開・再起動・容量試験の成功になりません。

## 正例と負例の境界

manifestのexpectedOutcomeは来歴hashに含めます。負例の422はnegative-* journalへ既知応答として保存し、追加診断失敗や再起動後の改変を成功にしません。正例の件数と負例の件数は別集計です。負例も同じ時間/RSS/disk budgetと実測に含まれます。現在の2正例は限定された横書き通知の資格であり、縦書き・表・任意のActualText置換を含む全PDFの対応を示しません。新しい条件を解釈できない場合の拒否を維持します。

## 小量成功の限定証拠ファイル

同一repositoryの専用label付きPRで小量試験に成功した場合に限り、既存のpinned artifact actionが固定JSON 1ファイルを1日保存します。repositoryはpublicなので、公開してよい検証用UUID・code/corpus/runtime hash・数値件数/資源値・再起動前後の一時PID・DB/storageの一方向identity hashだけを明示抽出します。原本PDF・本文・snapshot・環境変数・credentials・任意log/pathは含めません。privateなreport.json全体はuploadしません。

JSONは小量正例2/負例1の成功条件、数値、UUID、再起動identity/PID、取得時刻を厳格に検査し、最大1MiB、固定パス、freshなdirectory/排他的file作成を使います。PRのcheckout headと一致しなければexport失敗です。fork PR、失敗run、大量stageはexport/uploadしません。公開ログにはreceiptのSHA-256だけを追加します。

SHA-256は整合性確認であって署名/実行真正性ではありません。verifyQualificationReceiptは信頼された別経路のcode/corpus/runtime/runId/SHA-256を全て要求し、authenticityVerified:falseを明示します。次段の利用前に、同repoの承認workflow、run/attempt、checkout head、artifact originとログdigestを照合する必要があります。未知JSONを自己申告hashだけでadmissionへ渡しません。今回の追加は保存/整合性検査までで、任意receiptの自動restoreや大規模CI起動を追加しません。

## 明示的な1,000件枠

`Document 1000 qualification` workflowはmanual dispatchだけを受け付けます。同一public repositoryのmain・固定1000入力・標準ubuntu-24.04に限定し、通常CIの45分枠は延長しません。job上限180分の内側で、fresh smallを5分以内、1000を120分の作業中止/admission期限、全chainを125分期限で評価します。期限後の既存probe/cleanupには時間を要する場合があり、runner全体の強制終了は180分です。

同一build/runtime/corpus/run UUID/DB/storageでsmallを取り直し、成功後のidentity・PID連続性を確認してから、現在の実測資源とfull small reportを既存admissionへ渡します。以前の添付receiptや別runnerの値を現在容量へ読み替えません。stage別のFolder名・directory/journal・再起動log世代を使い、smallの文書を1000件へ含めません。factor2、RSS2GiB、disk余裕1GiB、空きmemory余裕512MiBは不変です。

成功時だけsmall＋1000の閉じたschemaの証拠を固定JSON1ファイルへ保存します（最大1MiB・1日）。旧small-only receiptの形式は維持します。未知field、別run/DB/storage、PID不連続、重複ID、未完の段階を拒否します。失敗/未admissionでは成功artifactを作らず、公開logへ固定reason codeと数値予測/予算/資源だけを出します。raw report・本文・原本・env/credentials/pathは出しません。

10,000/100,000はこのworkflowに選択肢を設けません。次の段階は実1000結果と使用環境を改めて評価してからです。詳しい停止条件と実装順は[計画](../../docs/superpowers/plans/2026-10-09-document-load-thousand.md)を参照してください。

# Document 段階負荷検証の状況

Status: ACTIVE / 実 API 資格は未取得

## 対象と承認

2026-10-08 の承認済み方針は、公式 PDF の少量試験から始めて 1,000 → 1万 → 10万件へ段階的に増やすこと。製品の性能 SLO や本番資格は設定しない。基点 main `8d4b94a912f9ac2e8079fc1c13b4efa4ce6d7538`、branch `test/document-load-qualification-20261008`。

[実行手順・検証範囲](../../../tools/document-load-qualification/README.md) と `tools/document-load-qualification/official-sources.json` が再開入口。共有 active pointer・導入手順・製品 source は変更しない。

## 既存作業との分離

- 既存 Document seed は合成 5 文書、runtime は実 API/PDF・同一 DB/storage 再起動を既に扱う。新規は公式原本来歴・段階件数・resource admission・計測に限定
- Search corpus branch `feat/search-validation-corpus-20261006` head `e508398bc11e343b76fd2c27c12fc521774c2322` の計画/状況を読取確認。既存 1,000 文書試験を複製しない
- Search 修正 Draft PR85 head `18080d442e78fc6d14c3e15abba93333740a2884` は独立作業。変更も取り込みもしていない
- 旧 Search 記録では公式 PDF は 0 件、Linux aarch64/native PDFium が障害。Document の実公開は資格済み Linux sandbox と PDFium が必要で、Mac native unit test だけで置き換えない

## 準備・ローカル検証

- 厚生労働省の 2025-03-31 通知 2 件を 2026-10-08 に公式 URL から取得し、5頁182,988B / 2頁95,621B、SHA-256 を確認。原本は repo 外。利用条件と出典を manifest/runbook に保持
- Node built-ins、既存生成 SDK / BinaryTransportBridge を利用。依存追加なし
- resource 欠測、deadline、再起動 identity、初回 create の結果不明、OCC 409、権限拒否、全ページ/原本 hash 等を TDD で契約検査
- 独立レビューで 25 万 sample の variadic stack overflow、最終 observation 中の期限超過、別 filesystem の容量増分が min 値に隠れる反例を発見し、RED→GREEN で補修
- cloud の Node は 24.19.0（repository pin 24.21.0 ではない）。Rust / Docker / psql / mise はなし。これらの local 契約結果は実 runtime 資格ではない
- root `mise run verify:fast` は mise 不在で未実行。API 契約の Redocly は telemetry 無効化を確認してから実行する必要があり、未確認の外部送信は実行しない

## 未実行と次の action

- 公式 PDF の実登録・公開・拒否・実 HTTP 再起動：NOT RUN
- 1,000 / 10,000 / 100,000：NOT RUN
- exact-head hosted / 全 CI / merge：NOT RUN
- 初回の次の action：最終独立レビュー、固定 toolchain の契約検査、Draft PRへ保存し、専用 `document-load-small` label で同一 head の通常 CI＋公式 PDF small を実行。実結果をこの記録へ追記する
- small が実成功した後、その report と host 資源から 1,000 件 plan を確定する。前段失敗・資源不足・欠測は調査/判断が残るもので、全段階の完了とはしない

## 2026-10-08 06:59 UTC — 公開前の source 資格

- 独立レビュー：real small 試行へ GO。最終の CI shell 並び替えも再レビュー GO
- 新ハーネス51件、既存 runtime/API 契約と合わせて248/248成功、skip0。生成SDK/binary試験12/12成功。型build、Node構文、diff whitespace検査成功
- 最初の広域回帰は既存 visual-integration の連続shell guard 1件が失敗（247/248）。既存 visual設定の並びを保ち、新small設定をその前に置く限定補修で、既存testを弱めず再度248/248成功
- Redocly2.52.1の実装とroot mise.tomlの正式な telemetry/update notice opt-outを確認し、REDOCLY_TELEMETRY=off / REDOCLY_SUPPRESS_UPDATE_NOTICE=trueでAPI契約を実行した。外部telemetryへの許可は追加していない
- 公開直前mainはPR108統合の6a34de3f0904949daca304b9178ef5125ea13d82へ更新。GUI/既存文書の変更をそのまま取り込み、今回のsource境界を広げない。組合せの再検証とexact-head hostedはこれから

07:01追補：main6a34de3fをfast-forwardで保持した（初回commitは作者未設定で未作成だったため不要なmerge commitなし）。親の確認ではMac空き容量5.3GiBで、大規模local runはadmitしない。現在の実行候補はhosted smallのみ。Search用text/plain固定ingest.pyを読取確認し、今回のPDF API受入clientとの非共有境界をREADMEへ明記した。

07:02追補：main6a34de3fとの組合せで生成SDK型buildと新規＋既存runtime/API契約248/248成功、skip0を新たに確認。独立レビューGOはsmall試行のsource資格のみ。Rust/実DB/公式PDF公開・large stage・exact-head CIは引き続き未実行。

## 2026-10-08 07:26 UTC — PR110 の初回 hosted は Document step 失敗

- Draft [PR110](https://github.com/AIrisu-072/knowledge-platform/pull/110)。公開head `7b95dbc75a2cb93cc9d8e148a5f07be7d77ba468` / tree `97274afb1c58304e7333057e9b2210c1186cd69a` はレビュー済みlocal treeと一致。GitHub connectorで公開し、remote ref/treeをreadback照合した
- label前の通常CI37741316776はcancelled。専用`document-load-small` label後の[CI37741452488](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37741452488)のDocument [job113193385744](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37741452488/job/113193385744)はFAIL。固定依存/font/Chromiumの準備は成功、Real composition-root acceptanceとsummaryはFAIL、Organization後続はSKIP
- jobログ取得はconnectorで3回ともTransport closed、artifactは0件。現時点では既存受入と公式PDF段階のどちらが失敗したか不明であり、PDF公開の製品不具合とは断定しない。許可された別executorでread-onlyログ取得を調整中。推測による製品変更や大規模実行はしていない
- 次のexact action：実ログの失敗stageと固定Problem/operationを確定し、所有境界内の不具合ならRED→修正→独立review→同PR exact-head CIで再検証する。境界外なら親へ具体的な判断を返す

07:38追補：CI37741452488は終端。14job中12成功、Documentと集約required-checkのみ失敗。PR110はDraft/open、head7b95dbc7を維持しmergeable=true。本文log未取得のため原因は未確定、推測fix・CI再実行・large実行はしていない。残る依存入力はjob113193385744のログ末尾とdocumentLoadQualification集計である。

## 2026-10-08 07:50 UTC — 実ログ確認と限定診断の追補

親が許可済みMacから取得したjob113193385744のログによると、既存browser/runtime/HTTP restartは成功し、公式smallへ到達した。最初の文書はcreate201、detail200、publish422。表示されたdocumentCount2は目標であり、2件成功ではない。失敗時elapsed692.937ms、sampled peakRSS526,835,712B、disk増分0は未完了stageの部分観測で、性能成功の証拠ではない。

具体的な422 codeは初版adapterで破棄されていたため、同PRに次だけ追加する：固定Problem code/status、owned pending FileId1件のread-only DSI件数・原本binding診断、作成/初回公開/目標の別表示、未完測定label、最終観測。製品のPDF validator、公開品質規則、API error detailは変更しない。取得不能な診断は元の公開失敗を置き換えない。

ローカル原本構造のread-only確認では、001472933.pdfはFreeText注釈1件、001472934.pdfは注釈0件だった。これはpypdfによる補助観測であり、実workerの拒否理由とはまだ断定しない。原本を加工・注釈削除して通さない。診断のRED→GREEN後、型build・全Node回帰254/254成功、skip0。独立レビューと新headの実再試験はこれから。

07:54追補：診断追補は独立レビューGO。新規57件を含む全Node回帰254/254成功、skip0。SQLはschema/predicate静的照合済みで、実PostgreSQL実行は次hostedで確認する。登録/公開数はconfirmed*（成功応答をjournalへ保存済み）へ明記し、結果不明時に実件数が増えていないとは主張しない。まず同じ公式原本で実DSI理由を確定し、その後に正当な拒否ならnegative corpusへ分類する。注釈の除去や未説明422の期待成功化はしない。

## 2026-10-08 08:11 UTC — 診断headの実run、ログ入力待ち

診断head `9f9dc606f84c7af023b11c0e9ba7a1727fc276f0` / tree `478d0c596dc5d1855dfcbb71210450d6e5ac3fd0` を同PRへ保存しreadback一致。既存labelにより[CI37746426778](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37746426778)を実行し、Document [job113208886558](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37746426778/job/113208886558)はcomposition-root/summaryでFAILとなった。今回もcloud connectorのログ取得はTransport closedで本文未取得。failureDiagnostic / inspectionDiagnostic / confirmed countsの実値は許可済みMacのread-only取得待ちで、未説明422を期待成功にはしていない。

次のexact actionは、このjob末尾のdocumentLoadQualification JSONを受領し実DSI拒否理由を確定すること。正当な品質拒否が実証されれば原本/出典を保存したままnegative corpusへ分類し、注釈のない公式PDFをpositiveの登録/版更新へ使う。次候補001472935.pdf（30頁1,008,343B、sha256 7e906b176e1ba50e34c6be6fbba37569238c404549de20b490b4a27ce2efed8b）は公式ページから取得済み、pypdfの補助読取で注釈0・署名field0。実DSI/公開はまだ未資格。製品validatorや注釈そのものは変更しない。

## 2026-10-08 08:43 UTC — BUSINESS_RULE_REJECTED と missing DSI の診断

許可済みMacで得たjob113208886558の実JSONは `operation:publish / httpStatus:422 / problemCode:BUSINESS_RULE_REJECTED`、`inspectionDiagnostic.status:not-found`、目標2/confirmed create1/publish0だった。PUBLISH_QUALITY_REJECTEDではなく、FreeText注釈を原因と扱わない。

公開serviceはWORKING/分類状態を確認した後にDSIを行い、既存synthetic PDF受入もcreate直後に同じpublish APIを使う。別の検査API呼出しが必要な契約ではない。一方、InspectionFailedのrequires_ocr・format_mismatch・semantic_extraction_failed・unsupported_semantic_construct等はHTTPでBUSINESS_RULE_REJECTEDへ集約されるため、保存行が無い理由は現時点では未確定。

親承認の限定driverを専用toolsに追加し、既存runner Cargo.tomlへexample登録だけを行う。新dependency/lock変更なし。既存optional runtimeがbuild/test/hashを固定し、元のLinuxSandboxRunner・worker・PDFium・10秒上限を再利用する。保存FileIdのhash/size/media/WORKING/分類状態を先に照合し、元API失敗を保持したまま固定worker codeだけを採取する。失敗時sandbox bypassはない。

Node契約の欠如RED→GREEN。独立レビューでDB size binding不足を見つけ、size照合必須のRED→GREENで補修した。独立レビューGO。新規62件、既存visualと合わせ77件の独立検査成功。全Node回帰259/259成功、skip0。Rust source/APIの静的照合は済んだが、cloudにRustが無いためcompile/format/2単体testと実sandbox実行はhosted待ち。原本の負例分類や製品規則変更はまだしない。

## 2026-10-08 09:16 UTC — harness診断準備済み、製品coverageでBLOCKED

Macタスクの実結果をread-onlyで確認：d217f51d/run37752175251/job113227809431は、保存前提がすべて一致した上で、同じLinuxSandboxRunnerが `unsupported_semantic_construct` を返した。APIは422 BUSINESS_RULE_REJECTED、登録1/公開0、DSI行なし、qualification:false。注釈が原因とは断定しない。

CIは終端14job中12成功（Rust test/static・両macOS parity等を含む）、Documentと集約required-checkは失敗。別のDSI PoC37752175211 / Sandbox Preflight37752175213は成功。これをsmall/大量/本番資格の合格とはしない。PR110はDraftのまま、mainへmergeしていない。

最新main b1c5c36d（PR109）をlocal worktreeへconflictなしでno-commit mergeした。まだ公開しておらず、この組合せの資格も未取得。PDF worker/runnerとversioning preflightは診断headとmainで同じ。製品coverageは変更していない。

再開先は[限定設計提案](../specs/2026-10-08-document-official-pdf-coverage-proposal.md)。利用者承認前はread-only/designのみ。推奨は既存PDFium/lopdfで意味を検証できる描画・構造の限定拡張、rasterは検証oracleのみ。一般PDFを単純なfixtureへ置き換えて実用成功にしない。表/ActualText/clip/新Difffield/cacheを落とさず、意味profile変更が必要なら別承認を求める。1,000以上はNOT_ADMITTED/未実行のまま。

## 2026-10-08 13:54 UTC — 限定PDF対応の承認とtest-first再開

13:49:25 UTCの「確認待ちの4点は進めてください」により、提示済みA（既存エンジンで文字中心PDFの描画・タグ意味を限定対応、Diffも検証、未知構造は拒否）の承認を受領。main114 `7f5dd26bb96d65f1dd478e644e9480d8666ebb7d` を競合なく保持した。旧成功corpusのfingerprint/evidence不変を互換条件にし、新profile・旧版identity移行が必要なら別判断を求める。

最初の変更はPDF paint契約のtest-only5件（default状態不変、vector形状・色の差、同値数値/default、包含clip）であり、製品codeは未変更。PDFiumの別process rasterでfixture表示差も検証する。cloudにRustがなく、Macの正確sourceによるRED確認待ち。smallの前回FAILと大量未資格は維持する。

14:13追補：Macでtest-only head a7f759feのactual REDを確認。固定Rust1.98.1/PDFiumでcompile成功、既存6成功・追加5失敗（UnsupportedSemanticConstruct）・既存raster子test1 ignore。旧main114の15合成PDFbaselineは10成功full evidence＋5拒否として別保持。これはLinux sandbox資格ではない。

最初のgraphics incrementはfinite opaque path/default state・vector projection・矩形clipの包含証明を追加する。独立レビューでDefaultGray/RGB override、stroke-text状態の欠落、vectorで一部textを隠す順序曖昧性を発見し、限定拒否と回帰fixtureを追加した。文字/Formとvectorの交差は初段拒否。strokeはsegmentの保守的bounds、clipは任意epsilonなし、proof回数にも上限を設ける。固定PDFium151での新fixture実行と旧baseline比較は次Mac試行待ち。タグ15件はtest-onlyで、gs/tag/Diff製品対応は未実装。公式smallは引き続き失敗/未資格である。

14:26追補：Mac整形/タグ試験compile修正head9494e68c3c313825a9e4d719e07eb24fa28fa191/tree59775dac38672156f648630b667314226d4832ebをfetch/fast-forwardで保持。変更は4file rustfmtとVecに対する誤った.expect除去のみ。タグは実compile後15件すべてBDC未対応のRED、paint16成功・既存child1 ignore、旧15baselineは全field/拒否完全一致。初回タグcompile失敗はbehavioral REDとして数えない。

gs/page group4件、Diff page残余/新vector5件、native vector2件、派生cache4件をtest-onlyで追加準備。top-level残余は次のprivate seam試験で補う。Nodeは現在main114組合せでbuild成功、実行したload/runtime/API契約glob257件成功・skip0。旧259とはglobが異なるので増減比較しない。

## 2026-10-08 15:00 UTC — gs/タグ/Diffの初回GREEN試行前

184676efのMac実試験は全compile成功、gs4件RED、Diff単体4RED/1control成功、cache2RED/2control成功、native vector2RED（期待Partialに対しNone/Full）。00fa83b/tree d8eac7bbのMac formatter-only4file保存をfetch/fast-forwardし、製品挙動不変を保持した。

その実REDに基づき、opaque ExtGState/default page-group、独立font decodeとParentTree/MCID/ActualTextを検証する内部structure module、Diffのvectors/structureと未知残余fieldの未検証表示、派生cache世代を実装した。旧Version/snapshot/manifest/DSI profileは変更しない。

初回独立reviewはCMap範囲の巨大展開・decode前の出力増幅・空ParentTree keyの未計数を指摘した。未防御の危険fixtureは実行せず、完全な限定CMap grammarをhelper前に検査し、128KiB source、固定1–2byte code、16,384 unique mappings、1 BMP scalar/文字、事前出力見積を課した。ParentTreeはpairも課金し未所有keyを拒否。タグだけのページも色space/page-group/outputintent検査を必須にし、Artifact propertyを明示subsetにした。

修正後の独立scoped reviewは全指摘ADDRESS、first Mac試行へGO。structure.rs SHA256 dbf8bbfb7ac644a787394392ae90ff04b021a5a2ec8812a0f9b453d6cbc46fcf。11純粋resource/grammar試験とtag19試験は新source未実行。Diff/cacheも静的GO。次はexact headのMac compile/GREEN・旧corpus全比較・原本の補助native観測、その後Linux実sandbox/API small。資格済み/全PDF対応/大量合格とは扱わない。

15:18追補：5c59063のMacは全compile成功。paint16・gs4・structure11・Diff7・pdf_diff9・cache4成功、tags18成功/1失敗（未閉鎖EMCがnested MCIDのUnsupportedへ先に分類された）。旧15corpusは全field/拒否一致。3ac5f939のformatter-onlyを保持した。

残1件は有限delimiter prepassでParserDisagreementを先に確定し、正しく閉じたnested MCIDの拒否を維持する。934のclip/overlap拒否に2つの固定診断categoryだけを追加し、条件やcodeは変えない。独立reviewは次Mac試行GO。公式933の補助native固定理由はtagged font encoding is unsupported（実4/5頁はIdentity-VおよびToUnicodeなしType0）、934はunsupported bounded PDF graphics state。原本はhash一致・未変更。どちらもまだ公開資格ではなく、933を注釈拒否とは扱わない。

15:33追補：ac147fd5のMacはtags19・paint16・structure12すべて成功、旧15full baseline一致。934の固定理由はpdf_vector_text_overlap_unqualifiedと実確定。cloudのPDFium153による補助観測では、括弧curveの大きい制御点hullが文字boundsと2箇所交差するが、曲線を変更しない1回の数学的半分割による2hullでは分離できた。資格はpinned151の次試験で確認する。

次はtest-only5件：曲線分割で証明可能な正例/曲線変更の意味差、白いcurved fillによる真の隠蔽拒否、ページCropBoxでnew vectorが切れる拒否、包含CropBox不変。現sourceで3RED/2control成功を期待する。独立静的review GO。条件を緩めたり原本を書き換えず、有限な包含証明だけを改善する。

15:53追補：cf5f7168のMacは曲線2件のactual REDを確認。Crop試験は隔離raster helper内で失敗しsemantic assertion未到達だったため、semantic REDに数えない。pdfium-render0.9.4のall()が原点0のpage_size範囲を抽出することを確認し、assertを弱めず原点0の右側CropBoxと右下vectorへfixtureを修正。同一viewportの線なしrasterとの一致も要求する。

観測済みcurve REDに対してのみ、各cubicを固定2つのinterval-rounded制御点hullへ分割する証明を実装した。再帰・任意epsilon・canonical path変更なし、fillの全path boundsと真の隠蔽拒否は維持。全midpoint演算で外向き丸め、stroke幅のL1上界も外向きに評価する。3純粋算術試験を追加。独立reviewは次Mac試行へGO。新Crop製品guardは、修正版fixtureのactual semantic REDまで未変更。

## 2026-10-08 16:16 UTC — 934 native成功、Crop保護の最終試行前

69ebba44のMacはpaint20成功/Crop1失敗、graphics3成功。修正版Cropは両raster proof成功後、semantic拒否期待に対してadapter成功を返す実REDだった。旧15fixtureは全field/拒否一致。原本934はhash/bytes不変でnative helper exit0、capability7・comment/tracked-change/external/signature各0。Linux sandbox/API公開資格とは分ける。

actual Crop REDに基づき、新semanticsを使うページだけ、有限な親継承でMediaBox∩CropBoxを解決し、PDFiumのpage boundingとの一致・全paint包含を確認する。無効なCropBoxをnative fallbackで受け入れず、all()抽出frame外のText/Formも拒否する。旧成功入力のbypassは維持。継承/不正type/大きいCropとの交差/非zero原点frameの4回帰を追加。独立静的reviewは次Mac試行GO。

次はpaint25・tags19・graphics-state4・graphics3・structure12・Diff/cache回帰・旧15fullbaseline・原本934維持。focused Clippyと全hosted資格はまだ未実行。harnessには異なる正例原本2件が必要なため、933の負例を保持して比較可能な実際の公式通知を限定探索する。原本切出し/注釈除去/簡易PDFへのすり替えは行わない。

16:39追補：fbdd867eのMacはpaint25/tags19/gs4/workerPDF lib19/Diff lib7/cache4成功。pdf_diffの2正例だけNoneで失敗した。幅2・線開始x12・miter既定10に対する保守的boundsが左−8へ広がり、MediaBox0で拒否されるため、Diffへ到達していない。旧15full baseline一致、934native成功を維持。

9bc3aceのformatter-only3filesを保持。期待Partialは維持し、正例fixtureをx40へ移して両原本の実PdfAdapter成功を比較前にassertする。元のx12は固定clip拒否＋Diff None/変更なし/未検証regionの負例として残す。製品条件の変更ではない。独立reviewはこのtest-only試行へGO。新しい正例候補はMHLW2020-03-30医政発0330第2号（000616197、2頁125872B、sha256 57cfad9114727870f4e58e37f7fa07d572b86d53f0f5ce652c001912fc5ba3b7）で、未加工のnative資格は次試行待ち。

## 2026-10-08 17:00 UTC — native回帰成功、正例/負例API統合

42cacb06のMacはfocused88成功・0失敗（raster子1既存ignore）、旧15corpusの成功fingerprint/evidence・拒否全fieldが完全一致。公式934と000616197は未加工hash/bytes一致でnative成功、capability7とcomment/tracked-change/external/signature各0。d0faafe6は機械的Clippy9件とfmtのみで、全target Clippy/fmt・focused88・baseline15・公式2件を再確認して成功。Linux sandbox/API資格はまだ未実行。

933を明示的reject-unsupported負例として保持し、934と000616197をpublish正例へ区分するハーネスをTDD中。negative module45件は実RED→GREEN。API authoring snapshot/human-only policy、corpus役割hash、同bytesの偽content切替拒否、同一再起動での負例保持を追加し、cloud Node24.19.0の全119契約試験成功（pin24.21.0実資格とは別）。負例は作業版保持・公開なし・422固定code・DSI不在・資格済みrunnerの固定unsupported理由と原本bindingを必須とする。負例を公開成功件数へ算入しない。

次は全体独立review、保存exact headのLinux既存runtimeで正例2/負例1のAPI・再起動・資源実測、全CIを確認する。1000以上は未admissionで、native/契約試験を容量根拠に使わない。

17:10追補：全体独立reviewで、新規tag投影のページ横断段落ownershipとcatalog既定言語の省略、BDC関連付けtag名の過剰なidentity寄与、Artifact言語scopeの未表現を検出した。旧15互換性は維持し、まず現sourceの差分/不変/拒否テストを追加してMacのactual REDを確認する。製品修正前。これらが解決するまでnative88成功だけで最終資格にしない。

17:25追補：8dac8af9/tree c5018391のMac実試験は全4target compile成功。tags23成功/新規9 behavioral RED、paint29成功/子1ignore、pdf_diff12成功/跨頁段落差分1 RED（誤ってFull）、Diff lib10成功。期待した意味欠落を確認してから、structure.rsのみの限定修正に着手した。fmtは次headで機械的にまとめる。cloudの同headはharness119＋既存runtime177＋生成SDK/binary19＝315件成功、skip0。いずれもLinux API/容量資格ではない。

17:31追補：既存の実RED9件に対し、structure.rsへ有限ページmembership・lazy catalog Lang・非MCID文字の既定言語・関連付けtagのidentity除外を実装し、6つの小さな実helper budget境界試験を追加。独立reviewは設計一致を確認したが、不正なStructTreeRoot直下MCRがParentTree owner=rootで通る別経路を指摘した。ISO32000-1 Table322ではroot子はStructElemに限る。次headはこの不正rootのdirect/array回帰だけを新しい期待REDとして残し、他のtag修正GREEN・旧15全field・公式2件を同時検証する。rootのguardはそのRED確認後に追加する。graphicsの100万overlap照合ちょうど/超過の小型fixtureも追加した。

17:40追補：c69b26d5のMacはtag32成功/不正root直下MCR1件が実RED、paint30成功/子1ignore、Diff実13・lib10・workerPDF lib25成功。旧15全field完全一致、公式2件native成功と原本hash一致を維持、全target Clippy成功。新rootのarray形はdirectのassertで未到達だったため、2つの独立testへ分割し、bounded root scanで非StructElemをParserDisagreementとする6行guardを追加した。新しいPDF機能は増やさない。

小量成功の限定証拠保存を同repo/同承認検証の必要範囲として追加。public repositoryであることをconnectorで確認し、固定JSONだけにsynthetic ID/hash/count/metrics/restart証拠をallowlist抽出、1日保持・既存pinned action/permissionsを維持する。private report/PDF/text/log/path/credentialsはuploadしない。独立reviewでPR headとGITHUB_SHAの違い、fork PRのgate不足を検出し、実RED→GREENで修正した。ハーネス135試験成功、full受入/receipt実生成は次exact head待ち。Macで4Rust filesのfmtと最終回帰を行い、そのheadのLinux API small/全CIを確認する。

## 2026-10-08 18:59 UTC — Linux公開2件成功、metadataハーネス不適合を修正

19f6d234のCI37818914619は12job成功/Documentと集約required失敗。別DSI PoC37818914484・Sandbox37818914479は成功。Macで取得した実job113455282091の固定JSONは、正例2登録/2公開成功、負例1登録/期待422拒否/公開0を示す。停止点はupdateMetadataの422 VALIDATION_FAILEDで、PDF検査の失敗ではない。実再起動・最終資格は未完、artifact0件、1000以上へadmitしない。

原因はハーネスがloadQualificationOperationをmetadata直下へ更新したこと。normative logical-data-model-v0 §2.7とtransaction-consistencyの管理v0は、document_type/owning_department/category/extensionsだけを編集可としている。製品は正しく拒否していた。同じ422を契約testで再現し、extensions object配下へ移す最小修正と、そのnested値の読戻し確認を追加。OCC409や品質guardは不変。ハーネス全136件成功、次は独立review後の同PR exact headで実small全工程を再試験する。

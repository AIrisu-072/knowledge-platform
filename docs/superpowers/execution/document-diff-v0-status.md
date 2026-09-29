# Document Diff v0 — Capability Execution Status

## 2026-09-29 JST — D局所共通GREEN、Linux canary manifest修正後のhosted再確認

- 状態: **ACTIVE**。DIF-01〜13はA/B/C同一head hosted gate完了。DIF-14〜15は局所とD共通検証PASS。D最終標準CI・Linux PDF worker canary待ち。Frozen Design意味変更提案なし。
- branch/PR: 設計Draft PR #23 head `993cd094a09bdefcd4a9985b62e2a99aa9049b68`、実装Draft PR #24 remote initial D head `a5168221c9f0626cfb9056c548870bdac4838c05`。local修正head `03b67220f0eeda894e1d584aba67c3a54380acff`、未push。両PR未merge、PR #24未解決review thread 0。
- D共通: pinned PDFium pathで `CARGO_INCREMENTAL=0 mise run verify:fast` SUCCESS、Rust 636/636（既定skip 5）、fmt/check/strict Clippy/architecture/API lint PASS。局所DIF-14 PDF 6/6、DSI PDF回帰8/8、DIF-15横断3/3、Application縦断1/1、Postgres競合1/1。資源測定と合成fixtureのfalse unchanged/false change/locator error各0は下記とqualification ledgerに記録。
- Initial D hosted `a5168221c9f0626cfb9056c548870bdac4838c05`: DSI Sandbox Preflight `36523564697` SUCCESS、DSI PoC `36523564667` SUCCESS。標準CI `36523564675` はLinux専用`pdf_runner.rs`の`document-diff-runner`テスト依存宣言欠落で`rust-static` compile FAIL。無効headの残りはキャンセルした。`03b67220f0eeda894e1d584aba67c3a54380acff` でLinux限定dev-dependency/Cargo.lockを修正。最終標準CIとLinux実worker canaryは未確認。
- blocker / 次: 修正headのhosted gate。次のexact actionはこの記録をcommit/pushし、PR #24 exact headの標準CI、Sandbox、DSI PoCを確認する。PRはDraft維持、merge・deploy指示なし。

以下は旧checkpoint。現在の工程ではない。

## 2026-09-29 JST — C hosted GREEN、DIF-14〜15局所GREEN、Delivery D共通/hosted NEXT

- 状態: **ACTIVE**。DIF-01〜13はA/B/Cの同一head hosted gateまで完了。DIF-14 PDFとDIF-15横断評価は局所GREEN、D共通/hosted gate待ち。Frozen Design意味変更提案なし。
- branch/PR: 設計Draft PR #23 head `993cd094a09bdefcd4a9985b62e2a99aa9049b68`、実装Draft PR #24 remote C head `a182c42a49417dfc6ca0394f9dc33506a5d1ea70`。local D code head `268a66a91b41dd5285eb3efe37c612ef30b08b4b`、未push。両PR未merge。
- C exact-head: 標準CI `36521663888`、DSI Sandbox Preflight `36521663872`、DSI PoC `36521663869` は全て `a182c42a49417dfc6ca0394f9dc33506a5d1ea70` でSUCCESS。
- DIF-14: test-only RED `52beacbfcacb0c02b7ef5e4cf80f08047c682eb9` は未実装`PdfComparator`だけでcompile FAIL。GREEN `49cf67a087bc4c7c33a56fe9e22678252f877a56`。Linux workerのPDFium warmup・明示runtime path `ba93a7fa8bf17a18dec429e600eb5dfbfb3789dc`、矩形を確定できない視覚差をpage-level未比較へ残す補強 `268a66a91b41dd5285eb3efe37c612ef30b08b4b`。PDF 6/6、既存DSI PDF 8/8、対象strict Clippy PASS。Linux実worker canaryはD hosted待ち。
- DIF-15: 横断受入3/3、Applicationで実TXT worker→最終監査→新旧原本locatorを持つ対照表1/1、PostgresでWORKING更新と最終監査の行lock競合1/1 PASS。8形式の合成positive/noise/exact/unknown fixtureでfalse unchanged 0、false change 0、locator error 0。100,000行と1超過、candidate/change/source byte上限と1超過を検査。macOS受入プロセス実測は0.47秒、最大RSS 53,100,544 bytes。広い実文書corpusの精度ではなく資格fixture上の測定。
- blocker / 次: D共通`verify:fast`と同一head標準CI/Sandbox/DSI PoC、Linux PDF runner canaryを未確認。次のexact actionはpinned PDFium pathで共通検証を一度実行し、PASSしたD記録をcommit/pushしてhosted gateを確認する。PRはDraftのまま。merge・deploy指示なし。

以下は旧checkpoint。現在の工程ではない。

## 2026-09-29 JST — DIF-10〜13局所GREEN、Delivery C共通GREEN・hosted NEXT

- 状態: **ACTIVE**。A/Bは同一head hosted gateまで完了。CのDIF-10〜13は局所RED→GREEN、C共通検証PASS・hosted gate待ち。DIF-14〜15未着手。Frozen Design意味変更提案なし。
- branch/PR: 設計Draft PR #23 head `993cd094a09bdefcd4a9985b62e2a99aa9049b68`、実装Draft PR #24 remote B head `7dde25a56784f3dd79b0a994d869ce06bb32acad`。local C code head `6819aca7415ad7374cf4b789adaac9010db641c6`、未push。両PR未merge。
- DIF-10: RED `014a4cd`→GREEN `19343d0`。DOCX 6/6、core protocol 6/6、Application contract 9/9、対象strict Clippy PASS。安定段落IDによる移動＋編集、表cell位置、付随編集、未知OOXML未比較。
- DIF-11: RED `694bff0`→GREEN `934dfc0`。XLSX 7/7、対象strict Clippy PASS。value/formula、sheet/order/visibility、range/table/chart/image/link/external、計算cache等価、一意行reorder、重複行の曖昧性。既存pin `rxls 0.1.3` をDiffにpromotion。
- DIF-12: RED `5a9fb0f`→GREEN `d062668`。XLSM 4/4、VBA参照unit 1/1、共有DSI `xlsm_vba_semantics` 6/6、対象strict Clippy PASS。既存DSIのVBA正規化投影をformat-specific APIで再利用し、worksheetとmodule/procedure/referenceを分離。macro非実行、無意味な空白/コメント/大小文字は同一。新version parserなし。
- DIF-13: RED `1843f25`→GREEN `54a9358`。PPTX 5/5、重複shape曖昧性unit 1/1、共有DSI `pptx_semantics` 8/8、対象strict Clippy PASS。DSI資格済みformat-specific slide投影を再利用し、slide/shape/text/table/chart/SmartArt/image/link/notesを区別。theme/font/background/internal ID noise等価。曖昧shapeはslide未比較。
- C共通: pinned PDFium pathで `CARGO_INCREMENTAL=0 mise run verify:fast` SUCCESS。Rust 625/625（既定skip 5）、fmt/check/strict Clippy/architecture/API lint PASS。初回は資格済みDOCXをまだ未対応と仮定する旧worker-shell testがFAILし、実fixtureでのshell dispatchに更新した。PPTX link差分のresource budget failureも未比較に保存するよう修正した。ディスク逼迫時はCargo生成物のみ`cargo clean`し、追跡ファイルは保全した。
- blocker / 次: 同一head標準CI/Sandbox/DSI PoC、DIF-15の資源実測・全形式受入は未完了。次のexact actionはこの記録をcommit/pushして3つのhosted gateを確認し、その後DIF-14 PDF RED。merge・deploy指示なし。

以下は旧checkpoint。現在の工程ではない。

## 2026-09-29 JST — B hosted GREEN、DIF-10局所GREEN、DIF-11 NEXT

- 状態: **ACTIVE**。DIF-01〜09はDelivery A/Bの同一head hosted gateまで完了。DIF-10は局所RED→GREEN、DIF-11〜15未着手。Frozen Design意味変更提案なし。
- branch/PR: 設計Draft PR #23 head `993cd094a09bdefcd4a9985b62e2a99aa9049b68`、実装Draft PR #24 remote B head `7dde25a56784f3dd79b0a994d869ce06bb32acad`。local DIF-10 GREEN `19343d089747fe7f038ea7009d8348d5c94c1452`、未push。両PR未merge。
- B exact-head: 標準CI `36509161078` SUCCESS、DSI Sandbox Preflight `36509161061` SUCCESS、DSI PoC `36509161323` SUCCESS、すべて `7dde25a56784f3dd79b0a994d869ce06bb32acad`。
- DIF-10: test-only RED `014a4cda8c29d5ae45ab98e1bb7829ae57f39bfd` は `DocxComparator` とworker付随差契約の不在だけでcompile FAIL。GREEN `19343d089747fe7f038ea7009d8348d5c94c1452` でDOCX 6/6、core protocol 6/6、Application contract 9/9、対象strict Clippy PASS。補足fixtureの移動＋本文変更とtable cell位置は各々焦点RED→GREEN。DSI-qualified意味fingerprintをguardとし、確定位置がない範囲を未比較とする。コメント/編集由来は内容差分から分離する。新規parser versionはpromoteせず既存pinの`office_oxide`/`quick-xml`/`zip`のみ使用。
- blocker / 次: DIF-10のhosted gateはC最終headで確認する。DIF-15の資源実測・全形式受入は未完了。次のexact actionはDIF-11 XLSXのDSI資格fixtureとparser pinを確認し、test-only REDを作る。merge・deploy指示なし。

以下は旧checkpoint。現在の工程ではない。

## 2026-09-29 JST — A exact-head GREEN、B局所GREEN・hosted NEXT

- 状態: **ACTIVE**。DIF-01〜06のAはhosted gateまで完了。DIF-07〜09のBは局所RED→GREENと共通検証済み、hosted未判定。DIF-10〜15は未着手。Frozen Designの意味変更提案なし。
- branch/PR: 設計Draft PR #23 head `993cd094a09bdefcd4a9985b62e2a99aa9049b68`、実装Draft PR #24 remote A head `ddc3c1b6e94557ed13831ed343f5d86d5bc97cec`。local B code head `81086412cb1fa4dbfdff140f51ed81bd37391057`、B記録commit・pushは次。PRはいずれも未merge。
- A gate: 標準CI `36507722838` SUCCESS、DSI Sandbox Preflight `36507722786` SUCCESS、DSI PoC `36507722787` SUCCESS、すべて `ddc3c1b6e94557ed13831ed343f5d86d5bc97cec`。Linux rust-testでDiff runner隔離3件PASS。先行A head `f05f4f9` の標準CI `36507007629` はLinux試験の変数shadowとmise OSV Scanner署名者未設定でFAILし、`ddc3c1b`で修正・再確認した。
- DIF-07: RED `a2abff6`（A修正後 `9bb61cd`）、GREEN `2379212`。TXT 5/5、shell 4/4、strict Clippy PASS。CRLF/NFC、変更・追加・削除のraw位置、ambiguous decodeの未比較。
- DIF-08: RED `373df77`、GREEN `0fbb26a`。CSV 6/6、strict Clippy PASS。quote noise、cell/row/column locator、一意なreorder、重複・不整合・delimiter曖昧性、資源上限理由を確認。
- DIF-09: RED `f4760b0`、GREEN `8108641`。HTML 4/4、strict Clippy PASS。visible text/link/imageと見出し/表構造、装飾noise、script非実行、意味が空のページを未比較とする。parser promotionはTXT `encoding_rs`/Unicode、CSV `csv 1.4.0`、HTML `html5ever`/`markup5ever_rcdom 0.39.0` の既存pinのみ。
- B局所共通検証: pinned PDFium pathを指定した `CARGO_INCREMENTAL=0 mise run verify:fast` SUCCESS、599/599 Rust tests（既定skip 5）、fmt/check/strict Clippy/architecture/API lint PASS。ディスク容量のため、完了済み旧worktreeの生成物に `cargo clean` を実行して空きを確保した。追跡ファイルは変更していない。
- blocker / 次: B hosted exact-head標準CIとDIF-15資源・大入力測定は未完了。次のexact actionはB記録をcommit/pushし、PR #24のB head CIを一度確認してからDIF-10 DOCXのRED試験へ進む。merge・deploy指示なし。

以下は旧checkpoint。現在の工程ではない。

## 2026-09-29 JST — DIF-01〜06 Delivery Unit A local GREEN、hosted gate NEXT

- 状態: **ACTIVE / A局所実装済み / hosted未判定**。DIF-01〜06のRED→GREENをcommitに記録。DIF-07〜15は未着手。設計意味の変更提案なし。
- 実装branch: `feat/document-diff-v0@ebd58f66b1bb3ab94c9f067a4a3b5fa991a1dda5`。設計・計画Draft PR #23 head `993cd094a09bdefcd4a9985b62e2a99aa9049b68`、未merge。A記録commit・push・実装PRはこれから。
- DIF-04: RED `4ca474a`、GREEN `4555645`。二原本FD worker/runner、portable shell 4/4、macOS拒否1/1。Linux強制canaryと資源profile候補はhosted qualification待ち。
- DIF-05: RED `95e5e34`、GREEN `4a2b851`。core alignment 4/4、Application service 7/7 PASS。曖昧対応を未比較として残し、現時点のpolicyとAuditを通して開示する。
- DIF-06: RED `3d6aef7`、GREEN `ebd58f6`。Application contract 8/8とprojection 1/1、Postgres cache 2/2とaccess実DB5/5 PASS。容量制限・digest照合・cache hitと表取得時の再認可/Auditを実装。
- A局所検証: `PDFIUM_DYNAMIC_LIB_PATH=<cached pinned PDFium> CARGO_INCREMENTAL=0 mise run verify:fast` PASS。Rust 584/584（既定skip 5）、fmt、workspace check、strict Clippy、architecture、API lintがPASS。初回はPDFium env未設定で既存DSI PDF試験1件がExtractorUnavailableとなったが、同版ライブラリ指定の焦点再実行と全体再実行がPASS。ディスク容量不足は回復済み。
- blocker / 注意: A exact-head標準CIとDSI Sandbox Preflightは未実行。PR #23の直近標準CIはrust-test等がSUCCESSだがsecurity jobがmiseの`github:google/osv-scanner@2.5.1` SLSA signer不在でsetup失敗。これはDiffコードの成否でなく、最終GREENには別途解消が必要。新Diff parser dependencyは未promotion。merge・deployは未指示。
- 次の exact action: この記録をcommit/pushし、PR #23をbaseにDraft実装PRを作る。A最終headの標準CI/Sandboxを一度確認し、失敗箇所だけ修正・再実行する。続いてDIF-07 TXTのRED試験へ進む。

以下は旧checkpoint。現在の工程ではない。

## 2026-09-29 JST — DIF-01〜03 local GREEN、DIF-04 NEXT

- 状態: **ACTIVE / Delivery Unit A 実装中**。DIF-01〜03は局所TDDを完了。DIF-04〜15とAのhosted gateは未完了。
- 実装branch: `feat/document-diff-v0@c042a27ec82738462a47d8d74c6d06c536e4af0a`（隔離worktree）。設計・計画Draft PR #23は未merge。実装PRは未作成。凍結設計blob `afee4e9351c5027b1252e8c7b74e542a295f0e67`、承認計画blob `0bdbba832ebcfc2d211fddbba18dc31e69e1a28c` を維持。
- DIF-01: RED `cbbe46d`、GREEN `0d886d7`。Application identity 5/5、core protocol 5/5 PASS。
- DIF-02: RED `a128d6a`、GREEN `b4bfca0`。snapshot実DB5/5、identity5/5、history回帰2/2、対象strict Clippy PASS。DSI欠落markerと再生成後のsnapshot再取得を実装。
- DIF-03: RED `724d73e`、GREEN `c042a27`。最終認可/Audit実DB4/4、file access回帰1/1、対象strict Clippy・fmt PASS。cache hitの再監査、policy剥奪、WORKING更新、T10、actor期限切れ、Audit失敗を確認。
- CI: DIF-01〜03のhosted exact-head CIは未実行。計画どおりA末尾で一度確認する。main baseline CI `36419479837` はsecurity setup失敗でありDiffの成否ではない。
- blocker / 判断: 現時点のDIF-04着手blockerなし。設計意味の変更提案なし。新Diff parser dependencyは未promotion。`executing-plans` helperはDIF見出しを解析できないため、承認計画を変えずledgerに手動記録している。
- 次の exact action: `feat/document-diff-v0` のDIF-04 briefに従いworker/runner crateの最小scaffoldと二原本・隔離のRED試験を先に置く。DSI公開sandbox sealを再利用し、Linux強制canaryはA hosted gateで確認する。

以下は旧checkpoint。現在の工程ではない。

## 2026-09-29 JST — 計画承認・実装開始

- 状態: **PLAN APPROVED / IMPLEMENTATION AUTHORIZED / DIF-01 NEXT**。DIF-01〜15はまだ未着手。
- 完了: 書面設計とProduction Implementation Planの承認。設計blob `afee4e9351c5027b1252e8c7b74e542a295f0e67`、計画blob `0bdbba832ebcfc2d211fddbba18dc31e69e1a28c`。承認の範囲はそれぞれの承認記録に従う。
- 使用branch / PR: `design/document-diff-v0@88722648db35407ca38599312f93164796bf8b90` はoriginと一致。Document Diff PRはまだない。実装branchは別のclean worktreeでこの計画承認headから作る。基準mainは `6ea29e1ceea82bb0e20b195890b7c0e7efc85f68`。
- 検証: `AGENTS.md`、Active/Status、凍結設計・計画、GitHub branch/PR/main CI、既存worktreeを再確認。main標準CI `36419479837` はsecurity setup失敗。Diff製品試験・CIは未実行。
- blocker / 未解決判断: 現時点でDIF-01着手を妨げるものはない。mainのsecurity setup失敗は最終GREEN判定までに別途解消・確認が必要。新parser dependencyの自動承認はない。凍結設計からの差分提案なし。
- 次の exact action: この承認/Statusを設計branchへcommit/pushし、Draft計画PRを作る。再利用する隔離worktreeに `feat/document-diff-v0` を承認headから作成し、DIF-01の焦点RED試験を先に書く。

以下は計画承認前のcheckpointであり、現在の開始条件ではない。

## 2026-09-29 JST — 書面設計承認、実装計画レビュー待ち

- 状態: **DESIGN APPROVED / PLAN REVIEW PENDING / IMPLEMENTATION NOT STARTED**。
- 完了: D1〜D13の設計合意、書面設計のcommit/push、依頼者による書面設計承認、Production Implementation Plan草案の作成と設計照合。
- 現在の工程: `docs/superpowers/plans/2026-09-29-document-diff-v0-production-implementation.md` の依頼者レビュー。DIF-01〜15はすべて未着手。
- 凍結設計: `docs/superpowers/specs/2026-09-28-document-diff-v0-design.md`、承認対象blob `afee4e9351c5027b1252e8c7b74e542a295f0e67`、設計提示head `design/document-diff-v0@42bd94be8737efbea1b289be15740c17e3e54398`。承認の範囲は設計承認記録に従う。設計意味の変更提案なし。
- 作業branch: `design/document-diff-v0`。Document DiffのPRは未作成。基準mainは `6ea29e1ceea82bb0e20b195890b7c0e7efc85f68`。PR #20は別CapabilityのSearch / Discovery設計PR。
- 検証: repository/GitHubのbranch、PR、main CIを再取得し、計画のspec coverage・型名・Task間の依存・Review Focusを自己点検。製品コード、migration、dependencyは変更せず、Rust試験・Diff CIは未実行。main標準CI `36419479837` は同headのsecurity setup失敗であり、Diffの成否を示さない。
- blocker / 未解決判断: 実装計画と実行方法は未承認。今回の依頼は設計のみであり、製品実装の開始指示はない。新production parser dependency、merge、deployの承認もない。
- 次の exact action: 計画書とこの記録を `design/document-diff-v0` へcommit/pushして依頼者に計画レビューを依頼する。承認・実装開始指示が得られた場合だけ、main/branch/PR/CIを再取得し、DIF-01のREDへ進む。

計画本文の資源数値は資格試験候補であり、実測済みproduction profileではない。設計凍結からの差分は提案していない。

# 文書詳細表示による既読・未読戻し：再実装の状況

Status: ACTIVE

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

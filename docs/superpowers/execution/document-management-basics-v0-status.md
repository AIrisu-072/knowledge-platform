# Document Management Basics v0 — 準備状況

## Active checkpoint — PR D MB-11/DMB-01〜25 実装head GREEN、2026-09-28 JST

- 状態: **IMPLEMENTATION ACCEPTANCE GREEN at PR D implementation head — 記録commitのexact-head gate待ち**。承認済み設計/計画blobは `38010802a04c285336810e9b9c637c656ed1a76b` / `3b5cc84a8593134cdd7e01ea026bd2a124fa9585` のまま。設計意味の変更提案なし。
- PR A #16 `e66fba6f56fec8e7666d8b1df667625e64ef2049`、PR B #17 `448d918ee3e1690695b8e02da4102d0f90e28664` は各exact-head標準CI/Sandbox/PoCがSUCCESS（run IDは前checkpoint）。PR C #18 head `72c4bb29847efe838ec24d63c75f3d5011a1b467` は標準CI `36399289710`、Sandbox `36399289754`、PoC `36399289760` がすべてSUCCESS。#15〜#19はOPEN/Draft、未マージ。
- C旧head `dc7f491` / `8c67d10` の標準CIは既存のT10/期限到達競合テストでFAIL。予約公開が先行した保存結果の再実行と、T10が先行したときの一時的なNotDue観測を試験が許容していなかった。製品状態遷移を変更せず、両競合順序を明示したテスト修正 `8c67d10` / `72c4bb2` の後、同一headの3 hosted gateがSUCCESS。
- PR D #19 は `feat/document-management-basics-v0-d@0f0b3d0322ac9c904a5dedb5f19b434fbb837bde`、base PR C #18、OPEN/Draft、未解決review thread 0。MB-11はT5〜T9の型付きDomain/Audit対応、T9/Version切替競合、T5→T7→T6→一覧→T9→履歴→T8剥奪の縦断、旧0005相当DB→0006〜0008移行とrollback/旧snapshot復元を実DBで確認した。新しい試験は既存契約の追加確認として初回GREEN。縦断試験の初回入力は設計が拒否する空明示policyを成功ケースと誤指定していたため修正し、製品REDとは記録しない。Performance fixtureは1,000 principal、10,000文書、1,000 Folder、最大深さ10でPASSし、公開一覧50件7,873ms、文書移動58ms、初回既読33ms（最終fixtureのmacOSローカル測定）。これは合意SLOではなく、一覧性能は改善検討事項。
- `mise run verify:fast` はfmt、workspace check/strict Clippy、architecture、API、543/543 Rust tests PASS（既定の除外5）。その後に旧予約行NULLと負荷fixtureのassertionを強化し、対象2試験PASS。最終差分の `mise run verify` もfmt、workspace check/strict Clippy、architecture、API、security、543/543 Rust tests PASS（既定の除外5）。`verify:full` は差分・repository policyが要求しないため未実行。D実装headの標準CI `36403750795`、Sandbox `36403750803`、PoC `36403750859` はすべてSUCCESS。受入証拠は `document-management-basics-v0-acceptance.md`。
- 本番identity resolver/transport実接続は未提供で、schedulerは未接続起動を拒否。Cedar／AWS Verified Permissionsは次期設計で検討。merge・deploy・本番migration指示なし。
- 次の exact action: Active/Status/受入記録のみをDへcommit/pushし、その新headの標準CI/Sandbox/PoCを一度確認する。結果をPR #19に記録してDraftレビュー待ちとする。mergeしない。

以下は前checkpointの履歴である。

## Active checkpoint — PR C MB-08〜10 local GREEN、2026-09-28 JST

- 状態: **IMPLEMENTATION ACTIVE — PR A/B exact-head GREEN、PR C local GREEN / hosted gate待ち**。MB-11とDMB-01〜25の全体受入は未完了。承認済み設計/計画blobは `38010802a04c285336810e9b9c637c656ed1a76b` / `3b5cc84a8593134cdd7e01ea026bd2a124fa9585` のまま。
- PR A #16 `e66fba6f56fec8e7666d8b1df667625e64ef2049` は標準CI `36382662702`、Sandbox `36382662689`、PoC `36382662759` SUCCESS。PR B #17 `448d918ee3e1690695b8e02da4102d0f90e28664` は標準CI `36387421101`、Sandbox `36387421076`、PoC `36387421029` SUCCESS。Bの旧head `0dce053` の標準CIは legacy孤立Folder試験が新single-root制約に反してFAIL。試験を自己参照の非rooted Folderへ修正した `448d918` で局所試験・全3 hosted gate成功。両PRはOPEN/Draft、未マージ。
- PR C branch `feat/document-management-basics-v0-c`。MB-08 code head `b675580`、MB-09 `3188980`、MB-10 `732a2680717b09835d76171a36275af872ff9072`。Cは未push・PR未作成。MB-08は未定義APIだけのREDから、複合主キーReadStateと初回監査の同一transactionを実装。並行/issuer分離/現行Version/T10/監査rollbackなど実DB7/7。MB-09はcursor未定義REDから、3種類の認可付き一覧とFolder直下一覧、NFC/literal検索、認可前のlimit禁止、本人未読、現在認可とcursor bindingを実装。cursor3/3、実DB6/6。MB-10は履歴/ファイルAPI未定義REDから、業務台帳とVersion fallbackの投影、旧予約終端NULLの保持、ContentItem/Representationの所属、監査commit後Storage openを実装。history2/2、file1/1、display name1/1。既存due7、schedule5、withdraw8、T10 transaction4の計24件PASS。対象strict Clippy/fmt/diff check PASS。Cのworkspace/exact-head CIは未実行。
- M-Cは `0008_document_read_state_v0.sql`。production追加dependencyは既存workspaceの `serde` と PoC-qualified `unicode-normalization 0.1.25` のみ。検索Indexやdurable共通IRは追加していない。設計意味の変更提案なし。
- 配備前提: 本番identity resolver接続は未提供でschedulerは未接続起動を拒否。Cedar／AWS Verified Permissionsは次期設計で検討。merge・deploy・本番migration指示なし。
- 次の exact action: この記録をCにcommit/pushし、PR B #17 baseのDraft PR Cを作る。C exact-head標準CI/Sandbox/PoCを確認し、成功後にPR D branchへ進みMB-11横断REDを作る。

以下は前回checkpointの履歴である。

## Active checkpoint — PR B MB-05〜07 local GREEN、2026-09-28 JST

- 状態: **IMPLEMENTATION ACTIVE — PR A exact-head GREEN、PR B local GREEN / hosted gate待ち**。MB-08〜11とDMB-01〜25の全体受入は未完了。承認済み設計/計画blobは `38010802a04c285336810e9b9c637c656ed1a76b` / `3b5cc84a8593134cdd7e01ea026bd2a124fa9585` のまま。
- PR A #16 head `e66fba6f56fec8e7666d8b1df667625e64ef2049` のexact-head標準CI `36382662702`、Sandbox `36382662689`、PoC `36382662759` は全てSUCCESS。OPEN/Draft、未マージ。PR B branch `feat/document-management-basics-v0-b` のMB-07 code headは `ca344d3f6ab662708a62d4804f0b0ded9ff51f2b`。Bは未push/PR未作成。
- MB-07 REDは `DocumentManagementService::move_document` と `FolderService::move_folder` 不在のみ。GREENではaccess排他guard、Folder ID順→Document ID順のlock、最新の旧/新親・Document権限、OCC、予約制限、cycle拒否、subtree全件の前後policy比較と旧policy administer再確認、access_revision、管理台帳とDomain/Audit outboxを原子commit。文書Version/原本/文書revisionの不要変更をしない。T10後はread_historyを追加要求してcurrent nullを保持。
- 実DB局所回帰: T5 9/9、Folder 7/7、Move 9/9、T8 6/6、Domain Folder 2/2 PASS。移動試験は元不一致、no-op、PENDING、T10、相互移動cycle、明示Document/子Folder policy維持、影響件数、移動後の再実行開示拒否、監査失敗rollbackを含む。対象crate strict Clippy、fmt、diff check PASS。新production dependencyなし。大きいsubtreeは15秒statement timeoutで全体rollback。Bの全workspace/exact-head CIは未実行。
- Planの「影響する子1件だけadminister不足」は、allow-onlyの最近傍policy置換では実効policyが変わる継承子は移動Folderと同じ旧policyを使うため、通常の有効状態として構成できない。実装は全影響対象の旧policy administerを個別に再確認し、明示policyで影響しない子を除外する実DB試験を固定した。設計意味の変更はない。
- 配備前提: 本番identity resolver接続は未提供で、schedulerは未接続起動を拒否。依頼者選択によりCedar／AWS Verified Permissionsは次期設計で検討。今回のFrozen Designを変更しない。merge・deploy・本番migrationの指示なし。
- 次の exact action: このcheckpointをBにcommit/pushし、PR A baseのDraft PR Bを作る。Bのexact-head標準CI/Sandbox/PoCを一度確認する。成功後、Bを未マージのままPR C branchへ進み、MB-08の実DB REDから続ける。

以下は前回checkpointの履歴である。

## Active checkpoint — PR A exact-head GREEN、PR B MB-05/06 local GREEN、2026-09-28 JST

- 状態: **IMPLEMENTATION ACTIVE**。MB-01〜04はPR Aのexact-head CI完了、MB-05/06はPR Bでlocal GREEN。MB-07〜11およびDMB-01〜25の全体受入は未完了。設計/計画承認blobは従来どおり `38010802a04c285336810e9b9c637c656ed1a76b` / `3b5cc84a8593134cdd7e01ea026bd2a124fa9585`。
- PR A #16: `feat/document-management-basics-v0-a@e66fba6f56fec8e7666d8b1df667625e64ef2049`、OPEN/Draft、base `design/document-management-basics-v0`、MERGEABLE。exact-head標準CI `36382662702`、Sandbox `36382662689`、PoC `36382662759` は全てSUCCESS。標準CIの前2回は保存時刻のPostgreSQL精度差と、旧版migration回帰試験が新認可関数を適用しないfixtureのためFAIL。各失敗を焦点試験で再現・修正した後の最終headが上記であり、古いrunをGREEN根拠にしない。
- PR B: branch `feat/document-management-basics-v0-b`、MB-05 code commitはPR A修正を取り込んだ `43abf5e8adf676dfc45f414a9c133598f0f51de9`、MB-06 code head `cdf02419d021243bd00a1cece03976501d9ca354`。PR Bは未作成・未push。MB-05 REDは `DocumentManagementService` 不在のみ、GREENはT5実DB 9/9。未知キー保持、extensions全置換、重複/型拒否、同値no-op、古いrevision、PENDING実変更拒否、T10後の追加権限、現在認可付き再実行、同ID並行、監査失敗rollback、revision overflowを確認。
- MB-06 REDは `normalize_folder_name` / `FolderService` 不在のみ。GREENはDomain 2/2、Folder実DB 7/7。NFC/trim/大小文字区別/255 scalar、root保護、親不在、同名並行、監査rollback、revision overflowを確認。M-B `0007_document_folder_names_v0.sql` は既存名を変更せず、preflightがinvalid名・非正規化・衝突・複数root・孤立・cycleをID/分類だけで報告し、migrationは問題時停止。Folder書込停止中のpreflight/適用が運用前提。対象3 crateのstrict Clippyとfmt PASS。新production dependencyなし。
- 配備前提: 実identity resolverは未提供。schedulerは未接続で起動拒否する。依頼者はCedar／AWS Verified Permissionsを次期設計の検討対象とする方針を選択し、今回のFrozen Designを変更しない。merge・deploy・本番migrationの指示なし。
- 次の exact action: Bにこのcheckpointをcommitし、MB-07文書移動/Folderサブツリー移動の実DB REDを作る。MB-07 GREENとBの局所回帰後にDraft PR BをPR A baseで作り、Bのexact-head標準CI/Sandbox/PoCを一度確認する。

以下は前回checkpointの履歴である。

## Active checkpoint — PR A MB-01〜04 local GREEN、2026-09-28 JST

- 状態: **IMPLEMENTATION ACTIVE — PR Aのlocal実装完了、exact-head CI待ち**。MB-05〜11およびDMB-01〜25の全体受入は未完了。
- 設計/計画: 承認済みblob `38010802a04c285336810e9b9c637c656ed1a76b` / `3b5cc84a8593134cdd7e01ea026bd2a124fa9585` を維持。PR #15は `design/document-management-basics-v0@66a273629a0c0c62f8a5fc88a1bb88f12bcb1a39`、OPEN/Draft、標準CI `36371656531`、Sandbox `36371656506`、PoC `36371656508` はSUCCESS。基準mainは `55dc3d3a430c8f36e1db8277fee15c4429258466`。
- 実装branch `feat/document-management-basics-v0-a`。MB-01 `5de3646c6f34d4c0f96e05bb0a7c15b6025437f5`、MB-02 `5859c411492b575281e3a8277fbed177d44efb52`、MB-03 `7680a2da6d23d314252bc6918e8e6202a6639099`、MB-04 `d17925aa270dece0c5535889e0f19cff85d80858`。M-Aは `0006_document_management_access_v0.sql`。新production dependencyなし。
- MB-03: 版操作replayと孤立Folder policyの漏洩をREDで再現して修正。認可付き入口、業務transaction内のaccess guard、現在policyと版可視性を同じ読取statementで判定。初版WORKINGの編集取得、読取専用による公開拒否、T10通常取得遮断、検査中の剥奪を実DB試験8/8 PASS。既存 `publication_end_visibility` 3/3、`publication_end_guards` 3/3、`publish_transaction` 7/7、`versioning_transaction` 5/5 PASS。入口契約試験とstrict Clippy PASS。
- MB-04: 未実装の期限到達認可APIをREDで確認。identity解決をDB lock外で実行し、依頼者の現在Read+Publishを確定前に再確認。権限剥奪・検査中剥奪は `authorization_revoked` 終端、一時障害は同じIDで再試行、無効identityは `identity_invalid` 終端。二重workerの公開/監査は1回。実DB試験5/5、既存due/schedule合わせて17/17、Application契約1/1、scheduler試験2件とstrict Clippy PASS。監査には実際のrequesterとservice executorを分け、旧記録にexecutorを捏造しない。
- 未検証: PR Aのexact-head標準CI、Sandbox、PoC。Linux sandbox canaryは既定のignoreで、今回のmacOSローカル実行には含まない。Design Freezeの意味変更提案なし。
- 配備前提: 本番identity resolverは未接続。`DueScheduler::connect` は `IdentityResolverRequired` で起動を拒否し、信頼済みresolverを注入した `connect_with_resolver` だけが稼働可能。本番schedulerの配備は接続提供まで不可。これは今回のコード実装を偽装して完了扱いしないための境界。
- 次の exact action: この記録をcommit/pushし、設計PR #15 baseのDraft PR Aを作る。Aのexact-head標準CI/Sandbox/PoCを一度確認する。成功後、stacked PR B branchへAのrun IDを記録し、MB-05のREDに進む。merge・deploy・本番migrationは行わない。

以下は前回checkpointの履歴である。

## Active checkpoint — PR A MB-01/02 local GREEN、2026-09-28 JST

- 状態: **IMPLEMENTATION ACTIVE — MB-01/02 local GREEN、MB-03 next**。PR Aは未完成で、MB-03〜11およびDMB受入全体を完了扱いしない。この節は以下の開始準備より新しい。
- 設計/計画branch `design/document-management-basics-v0@66a273629a0c0c62f8a5fc88a1bb88f12bcb1a39`、PR #15 OPEN/Draft。exact-head標準CI `36371656531`、Sandbox `36371656506`、PoC `36371656508` はすべて SUCCESS。凍結設計/承認計画 blob は変えていない。
- 独立した実装branch `feat/document-management-basics-v0-a`。MB-01 commit `5de3646c6f34d4c0f96e05bb0a7c15b6025437f5`、MB-02 commit `5859c411492b575281e3a8277fbed177d44efb52`。Draft PR Aはまだ作成していない。
- MB-01: Domain契約 RED は未定義policy APIのみ、Application契約 RED は容量不足の一次試行後、生成物整理・再試行で未定義management APIのみ。GREENはDomain 5/5、Application 5/5。canonical JSON vectorとコマンドdigestは独立Python計算の固定hexと一致。対象crate strict Clippy PASS。
- MB-02: 未定義T8/Repository APIによるREDを確認。M-Aは `0006_document_management_access_v0.sql`。実PostgreSQLの `access_policy_transaction` 6/6 PASS（root fail closed、一度限りのbootstrap、nearest policy、予約中変更、再実行/現在認可、no-op、型付き監査、監査失敗の全rollback）。対象crate strict Clippy PASS。旧Document Audit行のresource type既定値も検査。
- 環境: 初回Application REDのビルドでディスク容量不足。完了済みT10 worktreeのCargo生成物を `cargo clean` で整理し、19GiB空きを確保して再実行した。ソースは変更していない。
- 未検証: PR Aのexact-head CI、`mise run verify:fast`、pin済みPDFium/Dockerの`mise run verify`、MB-03/04と後続PR。実装PRのCIを毎Taskでは起動しない。
- blocker: なし。新しいproduction dependencyなし。Design Freeze意味変更なし。
- 次の exact action: 実装branchをpushし、MB-03の認可付き既存経路をRED試験から実装する。既存業務transaction内のaccess guard、現在認可の再実行開示、T10通常参照遮断の回帰を先に固定する。MB-04までGREENになったらPR AをDraft作成し、まとまったheadでexact-head CIを確認する。

以下は前回checkpointの履歴である。

## Active checkpoint — 計画承認・実装開始、2026-09-28 JST

- 状態: **PLAN APPROVED / IMPLEMENTATION ACTIVE — MB-01 開始準備**。MB-01〜11はまだ未完了。この節は以下の旧準備記録より新しい。
- 依頼者は `2026-09-28-document-management-basics-v0-production-implementation.md` の blob `3b5cc84a8593134cdd7e01ea026bd2a124fa9585` を明示承認した。設計 blob `38010802a04c285336810e9b9c637c656ed1a76b` は凍結済み。計画承認記録は `docs/superpowers/plans/2026-09-28-document-management-basics-v0-production-implementation-approval.md`。
- 開発ログ一元管理は**依頼者の指示で今回の開始条件から除外**した。別プロジェクトの完了を確認したわけではない。文書管理側の監査・イベント・運用観測要件は維持。
- 実行方式: `superpowers:executing-plans` による MB 番号順の実装。設計PR #15→実装 Draft PR A→B→C→D。merge・本番配備・本番データmigrationの指示はない。
- 着手時の正本: `main@55dc3d3a430c8f36e1db8277fee15c4429258466`、PR #15 `design/document-management-basics-v0@35eb7bc72cb778d9a5689fe2db8410c468a9dd30`。PR #15は OPEN/Draft、未解決 review thread 0、同 head の required checks は SUCCESS。承認対象の設計・計画 blob はこの head と一致。基準 main との差分は計画文書のみで、既存の同機能実装は見つかっていない。
- 規範反映: 承認済み設計 §17・計画 §7 に沿い、logical-data-model、transaction-consistency、observability-auditへ追記した。Versioning の期限到達認可と T10 の履歴専用経路は別の接続文書へ記録し、既存承認記録を維持。製品コード・migration・依存はまだ変更していない。
- 検証: 参照元 blob、PR #15 head/diff/checks、main head、関連 spec を確認。規範追記の差分レビュー中。Rust/実DBは未実行。設計PRの最終 head CI はcommit/push後に確認する。
- blocker: なし。設計意味を超える差分が見つかればその箇所のみ改訂gateへ戻す。
- 次の exact action: 規範追記の意味差分を確認し、この承認・規範記録を設計ブランチへcommit/pushする。独立実装worktreeを設計headから作り、MB-01の契約試験REDを作る。

以下は計画承認前の履歴記録であり、現在の開始条件を示さない。

- 状態: **DESIGN APPROVED / PLAN REVIEW PENDING / IMPLEMENTATION BLOCKED**
- 日付: 2026-09-28 JST
- 対象: transaction T5〜T9、認可付き一覧・版/操作履歴・ファイル参照、必要なT11/T12。
- 設計: `docs/superpowers/specs/2026-09-28-document-management-basics-v0-design.md`
- 設計承認: `docs/superpowers/specs/2026-09-28-document-management-basics-v0-design-approval.md`
- 実装計画: `docs/superpowers/plans/2026-09-28-document-management-basics-v0-production-implementation.md`
- 設計・計画ブランチ: `design/document-management-basics-v0`
- PR: #15 — Draft / Open / 未マージ。最新headとchecksはGitHubから再取得する。
- 基準main: `55dc3d3a430c8f36e1db8277fee15c4429258466`
- 承認対象の提示head: `0314f91a4e68221ed06778d36eaf228d0adfec86`
- 凍結した設計本文blob: `38010802a04c285336810e9b9c637c656ed1a76b`。本文は変更していない。

## 承認・実装開始条件

| 項目 | 状態 |
|---|---|
| 書面設計 | 依頼者承認済み。承認記録を参照 |
| Production Implementation Plan | 作成済み・レビュー待ち。未承認 |
| 実行方法 | 計画レビューで確定。過去T10の実行方法を自動流用しない |
| 開発ログ一元管理 | 完了未確認。別作業の状態を推測しない |
| 製品実装開始 | 未指示・未着手 |
| 規範差分の反映 | 計画で追跡。実装前に差分レビューする |
| merge | 指示なし |

## 今回の準備内容

- 書面設計の承認対象をcommit/blobで特定して記録した。
- PR A（認可と既存操作）、B（属性・Folder・移動）、C（既読・一覧・履歴）、D（T11/T12統合1本）の4単位、MB-01〜11の実装計画を作成した。
- 対象ファイル、共通型とport、DB保存単位、digest/cursor、RED/GREEN手順、25受入条件の対応、migration/復元とexact-head gateを記載した。
- 各機能で必要なイベントと原子性試験を完成させる。後続Dを未記録機能の後付け工程にしない。
- 計画レビュー対象の技術上限・DTO・history代表版・nullable終端証跡を計画第7節に列挙した。

## 検証の記録と限界

- 作業開始時GitHubのmainとPR #15は上記SHAだった。旧PR headのCI run `36367663785` は取得時in_progressで、SUCCESSは未確認だった。更新後headの証拠には流用しない。
- AGENTS、active、T10 status/設計/計画、今回の設計全文とstatus、Domain型、PostgreSQL modules/test fixture、scheduler assemblyを参照した。
- ローカルcloneは実行環境でgithub.comの名前解決に失敗した。GitHub connectorの読み書きは使用可能。ローカルRustテスト、実DB試験、配備検証は未実行。
- 計画の構造・MB-01〜11の順序・DMB-01〜25の対応・65個の未着手チェック欄・canonical vector・空白/競合markerをローカルの文書検査で確認した。これはRust実装試験ではない。計画のローカルblob SHAとGitHub create_blobのSHAは `3b5cc84a8593134cdd7e01ea026bd2a124fa9585` で一致した。
- 更新後PRの実際の差分とhead/checksはPRコメントへ記録する。CI待ちを成功扱いにしない。
- 製品コード、migration、依存、OpenAPI、規範spec、既存active pointerは変更しない。

## 次のexact action

1. PR #15の最新headと差分を取得し、計画をレビューする。
2. 計画承認と実行方法の決定を記録する。未確認なら実装へ進まない。
3. ログ一元管理未完了なら `PLAN APPROVED / WAITING_FOR_LOG_CENTRALIZATION` として保持する。
4. 完了確認と実装開始指示があった場合に限り、最新mainとの差分・実採番・規範反映・環境・exact-head checksを確認する。
5. その時点で今回のCapabilityをactiveへ接続し、MB-01のREDから実装する。

自動監視・自動開始は設定していない。本文の過去PROPOSED表示は承認記録で更新された状態と区別する。MB-01〜11と25件の受入条件はいずれも実装・実行済みではない。

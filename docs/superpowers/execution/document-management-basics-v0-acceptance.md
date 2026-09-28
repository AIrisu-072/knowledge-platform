# Document Management Basics v0 — 横断受入証拠

## 判定境界

承認済み設計 `2026-09-28-document-management-basics-v0-design.md` と実装計画の DMB-01〜25を対象とする。PR #15→#16→#17→#18→Dの順で差分を読む。ここに記した局所試験は最終headのhosted CIに代わらない。Draft PRのmerge、本番identity接続、本番migration、HTTP/CLI/GUIは別の操作である。

PR C #18 のexact head `72c4bb29847efe838ec24d63c75f3d5011a1b467` は、標準CI `36399289710`、DSI Sandbox Preflight `36399289754`、DSI PoC `36399289760` がすべてSUCCESS。旧head `dc7f491` / `8c67d10` の標準CI失敗は、T10/期限到達競合テストが有効な競合順序を許容していないことが原因であり、保存済みPublish結果の再実行と一時的なNotDue観測を区別して修正した。製品の公開状態遷移は変更していない。

## T1〜T10のDomain/Audit対応

件数は成功した実変更1回当たり。T5〜T8の変更なし、同じ操作IDの完全再実行、T9の重複確認は追加のmutationイベントを作らない。必須イベント書込みは業務更新と同一transactionであり、commit不明は同じIDの保存結果を照会して扱う。

| 操作 | Domain Outbox / 件数 | 必須Audit / 件数 | 対象とrevision・保存単位 |
|---|---|---|---|
| T1 初版登録 | DocumentCreated + DocumentVersionCreated / 2 | document.created + document.version.created / 2 | Document。初版とFileObject、両Outboxを原子的保存 |
| T2 Version作成/更新/rebase | DocumentVersionCreated/Updated/Rebased / 各1 | document.version.created/updated/rebased / 各1 | Document、Version操作台帳、Document revision |
| T3 手動/期限到達Publish | DocumentVersionPublished / 1 | document.version.published / 1 | Document、Publish台帳、current/revision。期限到達は予約台帳も同時確定 |
| T3a 予約/取消/終端 | DocumentVersionPublicationScheduled/Cancelled/Terminal / 各1 | document.version.publication.scheduled/cancelled/terminal / 各1 | Document、予約/取消台帳。終端は新規記録のみ時刻・executorを保存し、旧NULLを推測しない |
| T4 取下げ | DocumentVersionWithdrawn / 1 | document.version.withdrawn / 1 | Document、Version操作台帳、current復帰またはnull、revision |
| T5 metadata実変更 | DocumentMetadataChanged / 1 | document.metadata.changed / 1 | Document、管理操作台帳、Document revision |
| T6 文書実移動 | DocumentMoved / 1 | document.moved / 1 | Document、管理操作台帳、Document revisionとaccess revision |
| T7 Folder作成/改名/移動 | FolderCreated/Renamed/Moved / 各1 | folder.created/renamed/moved / 各1 | Folder型の対象ID、管理操作台帳、対象Folder revision。移動はaccess revisionも更新 |
| T8 policy実変更 | AccessPolicyChanged / 1 | access_policy.changed / 1 | AccessPolicy型の対象、binding/policy revision、access revision、管理操作台帳。root初期化も独立した1組 |
| T9 初回既読確認 | なし / 0 | document.version.read_confirmed / 1 | DocumentVersionのReadState複合キー。Document revisionと管理操作台帳は不変 |
| T10 公開終了 | DocumentPublicationEnded / 1 | document.publication.ended / 1 | Document、公開終了台帳、current nullと予約終端、Document revision |
| ファイル開示許可 | なし / 0 | document.file.access_granted / 1 | Document/Version/item/representation所属を認可後、監査commitの後にStorageを開く |

`management_event_matrix` はT5/T6/T7/T8/T9の型・件数・再実行を実DBで確認する。既存 `repository_contract`、`versioning_transaction`、`publish_transaction`、`due_transaction`、`schedule_transaction`、`withdrawal_transaction`、`publication_end_transaction` がT1〜T4/T3a/T10を確認する。検索イベントは正本側Outboxであり、Search配送・Indexの実装ではない。

## 横断試験と移行

- `management_concurrency`: Version切替がT9に先行した場合、旧版を新規既読にせずReadState/監査が0。既存の `access_policy_transaction`、`scheduled_authorization_transaction`、`management_move_transaction`、`read_state_transaction` が剥奪・相互Folder移動・予約・同時書込を扱う。
- `management_vertical_slice`: T5→T7→T6→公開一覧→T9→履歴→T8剥奪の一連の入口を実DBで確認。空の明示policyは設計どおり拒否し、別主体だけへの置換後は一覧・履歴・古い操作IDの結果開示を拒否する。
- `management_migration`: 0005相当の旧DBを隔離して0006〜0008へ移行。失敗する0007を同一transactionに注入し、0006を含むrollbackと旧行保持を確認。成功移行後にPostgreSQLの旧DB template snapshotから隔離DBを再作成し、移行前schemaと既存Version/currentを復元した。稼働後の無損失down-migrationは主張しない。実運用の復元は書込停止、バックアップ復元またはforward fixを選ぶ。
- `management_performance` は通常CIでignoreし、合成データで実行。実測assertionは1,000 principal、10,000文書、1,000 Folder、最大深さ10。最終fixtureの測定値: 公開一覧50件 7,873ms、文書移動58ms、初回既読33ms、継承判定の `EXPLAIN ANALYZE` 1.066ms（macOSローカル、PostgreSQL 18.6コンテナ）。代表SQL plan nodeは一覧 `Limit`、移動 `ModifyTable`、既読の行ロック `LockRows`。容量上限や合意済みSLOではない。公開一覧は負荷時の改善検討事項として残す。診断出力は合成件数・時間・plan nodeのみ。
- 新しいMB11試験は既存契約の追加確認として最初からGREENだった。縦断試験の初回入力に承認済み設計が拒否する空の明示policyを成功ケースとして使っていたため、試験入力を修正した。この失敗を製品REDとは数えない。Cの競合回帰はhosted CIの実失敗に基づいて修正した。

## DMB-01〜25の証拠対応

| ID | 局所試験・確認対象 |
|---|---|
| 01–02 | `document_metadata_transaction`、`management_vertical_slice`：共通属性のみ、未知キー保持、重複/型/no-op |
| 03–04 | `document_metadata_transaction`、`management_move_transaction`：予約と移動、Version/既読保持 |
| 05–06 | `folder_management_transaction`、`management_move_transaction`：名前、root、cycle、継承影響の全件判定とrollback |
| 07–08 | `document-domain/tests/access_policy_contract`、`access_policy_transaction`：空明示拒否、最近傍置換、操作の非暗黙包含、issuer分離 |
| 09–11 | `access_policy_transaction`、`scheduled_authorization_transaction`：DB競合、剥奪、identity一時障害、予約ID再試行 |
| 12–14 | `read_state_transaction`、`management_concurrency`、`document_query_authorization`：明示確認、並行初回、旧版/新版の独立性 |
| 15–17 | `document_query_authorization`、`authorized_document_transaction`、`management_vertical_slice`：一覧認可とcursor、編集/履歴の権限、入口迂回防止 |
| 18–20 | `version_file_access`、`document_history_projection`、`publication_end_visibility`：所属/名称の秘匿、T10、Outbox非依存履歴 |
| 21–23 | `management_event_matrix`、各mutation transaction試験、`version_file_access`：Domain/Audit原子性、同ID再実行、監査前Storage open禁止 |
| 24 | `management_migration`、`folder_management_transaction`、既存Create/Get・Publish・Versioning・DSI・T10・scheduler回帰と全体verify |
| 25 | `targeted_events`・履歴/ファイルの出力allowlist、診断ログ差分。秘密値を性能出力へ含めない |

## 最終head gateと配備前提

`mise run verify:fast` は2026-09-28のPR D局所差分でfmt、Rust check/strict Clippy、architecture、API、543/543 Rust tests PASS（既定の除外5）。その後、旧予約行と負荷fixtureのassertionを強化し、対象2試験を再実行してPASS。最終差分の `mise run verify` もfmt、Rust check/strict Clippy、architecture、API、security、543/543 Rust tests PASS（既定の除外5）。`verify:full` は今回の差分・repository policyが要求しないため未実行。

PR D #19 の実装head `0f0b3d0322ac9c904a5dedb5f19b434fbb837bde` は、標準CI `36403750795`、DSI Sandbox Preflight `36403750803`、DSI PoC `36403750859` がすべてSUCCESS。PRはOPEN/Draft、baseはPR C #18、未解決review thread 0。以下の状態記録だけを追記するcommitは、別のexact headとして再確認する。

本番identity resolverとtransportの実接続はこのv0に含めず、schedulerは未接続時に起動を拒否する。Cedar/AWS Verified Permissionsへの移行は依頼者の選択により次期設計で検討する。PRはDraftのままレビューし、明示的なmerge指示を待つ。

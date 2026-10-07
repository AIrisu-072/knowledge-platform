# Organization 作業ファイル・共有provider・Handoff・差戻し後の作業（U3）— 小計画

設計：[実装追補](../specs/2026-10-07-organization-work-files-handoff-amendment.md)。基点：U2統合後のmain。TDD（RED→GREEN→変異確認）、合成データのみ。

## Task 1 — Domain（`work-domain`）

1. 試験 `tests/files.rs`（RED）：作成・内容登録（世代＝操作ID、大きさ・hash・上限）・受領確認なしの提出拒否・固定・受領者の取得、名前規則、担当者以外と担当変更後の非開示、外す、差戻し後の取込みと再提出・前回提出の不変
2. `files.rs`：`WorkFile`・`FileGeneration`・`GenerationInput`・`DerivedFrom`、名前・media type・SHA-256の検査、受領確認（非永続、`PolicyAuthority`）
3. `Command` 4種（作成・内容登録・外す・取込み）と `MutationResult` 4種、認可（`work.edit`、現在の試行の担当者）、`authorize_recovery`
4. 文案の `value` を省略可能に（文案のJSONは不変）。提出はファイルの世代と受領確認を要求し、`PinnedArtifact` に `file` を固定
5. 新しい誤り `WORK_ARTIFACT_UNAVAILABLE`

## Task 2 — Application port と Repository

1. `work-application`：`WorkArtifactStore`（put・verify・read）と `WorkRepository` の内容登録・取得の既定実装（DependencyUnavailable）
2. `work-repository-postgres`：store注入、内容登録（認可→preview→保存→command）、提出のpreview後に選択世代をstoreで確認してからlock、取得は認可→読取り・照合→再認可
3. migration 0009（stagingのaction語彙だけ）、staging payload（名前・本文なし）
4. 実PostgreSQL試験：ledger・staging・replay、受領確認の失敗で提出しないこと、異なるbytesの同一操作IDがOPERATION_CONFLICT

## Task 3 — Storage adapter と HTTP

1. `organization-server`：`FileSystemStorage` を `work-artifacts/` namespaceで使うadapter（既存世代の一致確認、読取り時の大きさ・hash照合）
2. `work-api-http`：ファイル作成（`file`）、`PUT .../content`（8 MiB、headerで操作ID等）、`GET .../content`、`discard`、`import`、snapshotのファイル取得。添付応答header。transport境界の応答上限を内容取得だけ拡張
3. HTTP試験：境界値、header、hash不一致で503、非開示
4. OpenAPIと生成型

## Task 4 — GUI

1. decoder（ファイル成果物・固定ファイル・新しい結果種別）とAPI client（binary送信・blob取得）
2. 作業ファイル欄：追加（8 MiBの事前確認）、内容登録の失敗・結果不明の再試行、取得、外す。文案は文案schemaだけを編集
3. 提出確認に文案とファイル一覧、受領・差戻前のスナップショットにファイルと取得
4. 差戻し後の「前回の提出を取り込む」
5. 試験（変異確認を含む）

## Task 5 — 実browser受入と文書

1. `files-journey.spec.ts`・`files-persistence.spec.ts`、runnerのstage（files-journey → files-restart → files-persistence）と閉じた診断語彙
2. 利用手順（日本語）、状況、active pointer
3. 独立review → 修正 → PR（U2統合後）→ exact-head CI → main統合 → 統合後CI

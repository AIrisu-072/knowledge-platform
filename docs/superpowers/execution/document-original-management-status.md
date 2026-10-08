# 原本構成編集・初回複数原本登録の進捗

2026-10-08。承認範囲は[設計](../specs/2026-10-08-document-original-management.md)、[計画](../plans/2026-10-08-document-original-management-implementation.md)。branch `feat/document-originals-20261008`、base `6440e026`、計画commit `defef0d7`。専用worktreeで実装し、rootの作業を変更していない。push/PR/main統合は親の調整待ち。

## 実装済み

- 作業版だけで原本追加・除外・取消・上下移動。最終原本の除外禁止。構成変更時だけordinal再採番。保持原本と補助ファイルの監査付き取得、UNKNOWN固定bytes再送、OCC・権限・世代guardを維持。
- 初回複数原本の新multipartと、全原本・Document・初版・domain/auditイベントの原子的DB保存。legacy単原本を維持。null/空/重複/欠落/未参照partsや不正path/mediaを拒否。
- ordered全File IDsの成功・UNKNOWN receiptと回復照合。部分一致を成功としない。GUIは一つの文書へ全原本を登録し、複数選択後に一件へ除外しても入力したpathを保持する。
- multipart準備を回復marker保存前に行う。送信前の容量拒否で回復不能UNKNOWNを作らない。schemaもlegacy/atomicの混在を拒否する。
- [日本語操作手順](../../operations/document-original-management.md)と実runtime受入sourceを追加。旧公開・全原本bytes・固定再送・公開切替・再起動snapshotを検証する試験を用意した。

## 実行した検証

Node 24.21.0、既存の固定依存を専用worktreeにhardlinkコピーして実行。offline installはtarball不足、通常installはDNS接続不可で成立しなかった。rootの依存を変更せず、依存の版やlockを更新していない。

- RED: 初回複数GUI新2件FAIL（既存19PASS）、SDK新2件FAIL（既存10PASS）、作業版構成helper/GUI不在、atomic command不在、HTTP新形式とnullmanifestの反例を確認。
- 最終全GUI: 正しい `apps/document-web` cwdで **1786件 / 75 suites PASS**。初回全GUIをroot cwdから実行した時の9件失敗はcwd依存の既存契約テストで、正しいcwdへ修正して全件再実行した。
- TypeScript application/SDK、schema生成check、GUI production build成功。webpackの既存size/runtimeChunk警告は残る。
- SDK **14件 PASS**、API contract **21件 PASS**。OpenAPI lint成功、Searchの既存localhost server警告1件。
- Rust focused **24件 PASS**: HTTP10、create application12、commit identity1、recovery validation1。
- PostgreSQL **18.6-bookworm** +実FSの専用試験 **1件 PASS**: 全2原本とdomain/audit各2イベント、部分/不一致/逆順の回復拒否、権限喪失、新しいrepository/storageアダプターでbytes保持、遅いoutbox CHECK失敗時の全DB行rollback。DBプロセスそのものの再起動試験ではない。
- strict Clippy（application/API/repository `--all-targets -- -D warnings`）、`cargo fmt --all -- --check`、`git diff --check`成功。
- 実runtimeの型検査・5試験収集・記録抑止source試験15件成功。実browser実行の成功を意味しない。
- registration/schema/SDKの独立レビューでP2を2件検出し修正、再レビューGO。Rust全体の独立レビューは親の実施待ち。

Rust/DBのRED・GREENとcontainer確認は担当agentのツール出力に記録したが、生ログファイルは保存していない。独立レビューで原ファイルを読み直せる証拠とは区別する。DB固有のREDは未記録。共有targetは `/Users/airisu/Documents/Codex/2026-10-08/task/knowledge-platform/target`、jobs2。最終focusedは `cargo test -p document-application --test create_document_contract --test create_outcome_validation --test commit_outcome_identity -p document-api-http --test create_http`、実DBは `cargo test -p document-repository-postgres --test initial_multiple_originals -- --nocapture`（`initial_multiple_transaction_full_recovery_and_adapter_restart`）。再現のための合成fixtureはrepositoryに保存しているが、生ログを後から作り直して元実行の証拠とはしない。

DBは所有する使い捨てcontainerだけを使用。最終container `18c43935…` は `127.0.0.1:32769` に束縛し、試験後の削除を確認。既存の利用者containerやセキュリティ設定を変更していない。

## 次のexact actionと未実施

### PR109 初回複数原本の公開500

Draft [PR109](https://github.com/AIrisu-072/knowledge-platform/pull/109)、保存head `39873ee47f2f2398b6d72b9fa5fa5218a82c18b9` のCI `37740624194` は新しい初回複数原本の実runtimeでFAIL。`initial-registration.spec.ts:187` の公開応答はexpected200 / actual500。登録・全原本取得・全IDs回復・WORKING確認までは成功し、既存の初回単原本と新しい作業版構成変更は成功した。これは合格やflaky再試行として扱わない。

原因調査で初回公開repositoryの `publish.rs` に `logical_path = 'primary' AND ordinal = 0` を要求する旧単原本前提が残っていた。新fixtureの正しい `appendix/B.txt` / 0 と `chapter/A.txt` / 1 ではtransactionのprimary存在検査が必ずfalseとなり、`IntegrityViolation` がHTTP500へ変換される。その後のtransaction loaderにも同じ固定anchorがあり、一箇所だけ除去しても解決しない。公開認可・OCC・replay・公開transactionを維持した上で、全canonical原本を検証し表示順の先頭を代表fileとして読む修正を行う。実runtimeの500をREDとし、新しいRust回帰試験とLinux実runtimeのGREENは修正exact-headのCIで確認する。

CIの親が保存したraw logは `/tmp/pr109-runtime-first.log`。既存の実DB独立再確認logは `/tmp/document-originals-independent-db-recheck.log`。この500の修正headのLinux資格と統合後資格はこれから。

根因の最小SQL再現を実PostgreSQL `18.6-bookworm`（image `sha256:3725f4e2499eef5134592b3b4ab79a543ed7f8e533b05b5b637af926630f6650`）で実施した。失敗headのproduction SQLを抽出し、二原本の完全な合成graphでも旧固定guardがfalseとなることを確認。修正sourceのproduction guardは同じ二原本と旧primary/0でtrue、先頭以外のfile欠落・representation欠落・空manifest・未classificationでfalseとなった。生logは `/tmp/kp-originals-publish-sql-red.log` と `/tmp/kp-originals-publish-sql-patch.log`、SQLは `/tmp/kp-originals-publish-repro.sql` と `/tmp/kp-originals-publish-sql-patch.sql`。SQL guardだけの検証であり、新しいRust試験・HTTP公開transaction・実browserのGREENとは扱わない。所有container `kp-originals-sql-20261008` / `976cf323…` のportは127.0.0.1限定をinspectし、検証後に削除した。既存DBを使用していない。

修正head `f8f53b5520aceef8b70361133091e34e113e844a` のCI `37743010441` は `rust-static` でFAIL。新しい回帰試験から `document-application` の `pub(crate)` な `DomainEventRecord::new` / `AuditEventRecord::new` を呼んだことによるE0624で、logは `/tmp/pr109-fixed-rust-static.log`。試験のAPI可視性の見落としであり、公開処理のGREENとして扱わない。イベント生成の可視性を変更せず、既存の公開 `DocumentService::publish_document` と `PublishDocumentCommand::new` を使う試験に修正した。OCC/replayと全原本の公開transactionは実repositoryを通し、異なるoperation payloadの再送拒否は既存service契約の `ApplicationError::OperationConflict` を照合する。製品sourceはこの試験修正では変更していない。独立sourceレビューで未classificationケースはcandidate読出時点の既存 `RepositoryError::BusinessRule` → `ApplicationError::BusinessRule` が正しいと確認し、このケースだけ期待値を修正した。空manifestはdocument/version存在確認を通過した後のcanonical joinがNoneとなり `IntegrityViolation`、先頭以外のrepresentation/file欠落はcandidate読出を通過して公開transactionの全件照合が拒否し `IntegrityViolation` となるため、他の欠落graph期待値は維持。新しいexact-headのRust compile/試験とLinux runtime CIは確認待ち。

head `957dee473e7532cce0a98f84e52dc4640f45758e` のCI `37744476249` / rust-test job `113202619185` は、新しい `nested_initial_originals_publish_with_full_manifest_and_replay` のSIGABRT / stack overflowでFAIL（595 PASS / 1 FAIL、途中停止）。生logは `/tmp/pr109-final-rust-test.log`。スタック上限は変更しない。新しい試験が複数の登録・公開service futureを直接awaitする構造を原因候補として、試験専用の通常関数でそれらを `Pin<Box<dyn Future>>` にし、test futureが保持するservice状態をヒープへ移す。各awaitで保持するfat pointerは2 machine wordsで、実service/repository/FSと全assertionは維持する。製品変更なし。この原因候補と修正の有効性は新しいexact-headの実試験GREENで確認が必要。

ローカルfocused `--no-run` を共有target / jobs2 / debug0 / incremental0で試みたが、空き容量が開始時7.0GiBから4.0GiB reserveへ下がったためSIGINTで停止（exit130）、projectのcompileと試験には到達していない。logは `/tmp/kp-originals-stack-build.log`。ローカルRED/GREENやコンパイル成功とは記録しない。修正後のfmt・diff検査は成功。

修正時の空き容量が約4.1GiBのため、4GiB reserveを維持する親の指示に従い共有target再buildを実行していない。新しいrepository回帰試験はnested二原本の初回公開、全順序付き原本の取得・immutable bytes、OCC、同operation replay・再利用拒否、全domain/audit/formal revision件数、欠落graphと未classificationのfail-closedを追加したが、この修正のRust compile/実DBGREENは未実施。既存CIの `rust-test` と実Document runtimeでexact-head資格化が必要。

共有Cargo cacheが増えディスクが4 GiB reserveを下回ったため、新しいheavy buildとbrowser harnessを止め、全Cargo終了を親へ通知した。親が所有する `target/debug/incremental` だけを整理し、5.2 GiBへ回復した。PR108統合main `6a34de3f0904949daca304b9178ef5125ea13d82` を通常mergeし、組合せhead `664847658c2b3d3413aa342ee3ec214e70257f7e` / tree `6254deb2fe3aee894c8e27125e0b9affd137154f` を保持した。製品treeは統合前と変わらない。

実browser harnessは `run.mjs` がqualified Linux sandboxとPDFium `libpdfium.so`を必須とするため、このMacではproduction実受入を資格化できない。無駄なbuildや確認の無効化をせず親へ通知した。次は独立Rustレビューと、Linux hosted exact-headで実Document browser journey・再起動persistenceを実行する。tools/document-poc-runtime/run.mjsとCIは別担当所有なのでこのbranchでは変更しない。

全workspace Rust、実browser、DBプロセス再起動、exact-head hosted CI、main統合後CI、対象PC導入、本番認証/TLSは未実施。実機や本番の成功を記録していない。既読の意味は変更しない。

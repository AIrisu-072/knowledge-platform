# 複数担当・役割・委任（U1）の小計画

- 基点：main `d515aa38`。設計の具体化は[実装追補](../specs/2026-10-07-organization-multi-principal-amendment.md)、進捗は[状況](../execution/organization-multi-principal-status.md)
- branch：`claude/trusting-knuth-dn5cx4`（1 PR。統合後は同名branchをmainから作り直して次単位へ進む）

## 手順

1. Domain：`organization.rs`（組織単位・役割・正式割当・委任、時刻つきの責任解決、policy操作、OCC）。Work集約の固定判定を工程の責任区分と現在有効な責任へ置換。担当変更commandと割当期間の記録。既存試験を維持し、意味が変わる4件は理由を記録して更新
2. Repository：migration 0007（policy集約table、ledger/stagingのtarget列と語彙）。Work操作はpolicy行share lock→workflow行update lock、policy操作はupdate lock。同一transactionでledger・staging。実PostgreSQLで同時引受・委任・担当変更・失効fencingを検証
3. HTTP/OpenAPI：session責任、units/roles、割当・委任の作成/取消、`tasks?actingAssignmentId`、`tasks/{id}/assignment`。生成型を再生成
4. Server：6 profile（固定port）、Document bootstrapの追加4名の閲覧grant（旧2名fixtureも受理）
5. GUI：実行担当の表示と範囲切替、担当の管理（非開示）と担当変更ダイアログ、「担当と委任」画面。結果不明は同一操作IDで回復
6. 受入：既存2名journeyを変えず、別の新DBで6 processの実画面journeyと再起動後persistenceを追加
7. 日本語手順、状況、active pointer。独立review→Draft PR→exact-head CI→main統合→統合後CI

## 検証コマンド

```sh
cargo fmt --all -- --check
cargo clippy -p work-domain -p work-application -p work-repository-postgres -p work-api-http -p organization-server --all-targets --locked -- -D warnings
cargo test --locked -p work-domain -p work-application -p work-api-http -p work-repository-postgres -p organization-server
WORK_POC_TEST_DATABASE_URL=… cargo test --locked -p work-repository-postgres --test postgres_transaction -- --ignored --test-threads=1
cargo run --quiet --locked -p architecture-lint -- check
pnpm organization:api:lint
pnpm --filter @knowledge-platform/document-web test
pnpm --filter @knowledge-platform/document-web exec tsc -p tsconfig.organization-runtime.json
node --test tools/organization-poc-runtime/*.test.mjs
mise run organization:poc:runtime   # CI（hosted）で実行
```

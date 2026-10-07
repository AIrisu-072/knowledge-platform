# 複数文脈・注意・表示Profile（U2）の小計画

- 基点：U1（[PR #96](https://github.com/AIrisu-072/knowledge-platform/pull/96)）統合後のmain。設計の具体化は[実装追補](../specs/2026-10-07-organization-work-context-attention-amendment.md)、進捗は[状況](../execution/organization-work-context-status.md)
- branch：`claude/trusting-knuth-dn5cx4`（U1統合後に同名branchを最新mainから作り直す。1 PR）

## 手順

1. Domain：合成文脈fixture（C1既存不変、C2：営業→審査、C3：営業→事務）、固定IDの一般化（各instanceの計画から次工程IDを決める）、審査工程の責任区分、試行の期限、Attention（新しい割当・差戻し・期限間近・期限超過）、確認済みの検証、文脈projection（表示名・進捗・履歴の開示条件）、表示Profileと選択規則
2. Repository：migration 0008（確認済み記録）、全instanceの有界な読取り、対象recordを持つinstanceの特定、操作のlock・ledger・stagingをinstanceごとに。追加文脈は明示の `seed-contexts`（`seed-work` は不変）
3. HTTP/OpenAPI：`work-view-profiles`、`work-contexts`（一覧・詳細・履歴）、`tasks/{id}/attention`、`attention-seen`、`tasks?contextId&workTypeId`、各責任の `workViewProfileId`、`WORK_CONTEXT_NOT_FOUND`
4. GUI：表示指定が無い場合の既定Profile、営業型の文脈一覧と概要、事務型のWorkType別キュー（自分の担当／引受可能／管理対象）、注意の表示と確認済み、審査Profileの初期module
5. 受入：6名policy段階の後、同じDBへ `seed-contexts`（2回目は無変化）→ 文脈journey → 6process再起動 → 文脈persistence。既存2名journeyは単一文脈DBのまま不変
6. 日本語手順、状況、active pointer。独立review → PR → exact-head CI → main統合 → 統合後CI

## 検証コマンド

```sh
cargo fmt --all -- --check
cargo clippy -p work-domain -p work-application -p work-repository-postgres -p work-api-http -p organization-server --all-targets --locked -- -D warnings
cargo test --locked -p work-domain -p work-application -p work-api-http -p work-repository-postgres -p organization-server
WORK_POC_TEST_DATABASE_URL=… cargo test --locked -p work-repository-postgres --test postgres_transaction -- --ignored --test-threads=1
pnpm organization:api:lint
pnpm --filter @knowledge-platform/document-web test
node --test tools/organization-poc-runtime/*.test.mjs
mise run organization:poc:runtime   # CI（hosted）で実行
```

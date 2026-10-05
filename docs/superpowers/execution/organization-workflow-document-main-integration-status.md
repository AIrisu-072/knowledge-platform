# Organization — 完了・保留再開・Document参照のmain統合

## 2026-10-05 01:21 UTC — 候補を準備、組合せの実受入待ち

基点はmain `b9097a436e1b33fc6f83b866476d8b6e1167598d` と[PR65](https://github.com/AIrisu-072/knowledge-platform/pull/65) `64b5ddb0017d46e3dbc9f2dcec4ed8f8cc1438b6`。前者を第一parent、後者を第二parentとして祖先を保持する。独立branchは `integrate/organization-workflow-document-main-20261005`。既存PR/branchは閉鎖・削除しない。

### 受入済みの範囲

- mainは[PR62](https://github.com/AIrisu-072/knowledge-platform/pull/62)合成Agent統合head `700e1efe` を取り込んだ。[通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37248971575)・[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37248971568)・[Sandbox](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37248971591)成功。実DB・2名Agent操作・両HTTP server再起動・復元・cleanup成功。初回 `c42f0d97` の再起動後GET失敗は原因未特定であり、製品修正済みとは扱わない。失敗時だけ有限のHTTP status/endpoint分類を残す既存診断を保持する
- PR65は[PR63](https://github.com/AIrisu-072/knowledge-platform/pull/63)の最終完了と[PR64](https://github.com/AIrisu-072/knowledge-platform/pull/64)の保留/再開を祖先に含む。[通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37249051067)・[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37249051069)・[Sandbox](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37249051128)成功。実DB/transaction・2名の根拠/判断/Agent/差戻/再提出/完了/保留再開、公開改訂と原本Downloadのsize/hash・Task無変更、両HTTP server再起動・保存復元・cleanupまで成功。公開artifact0
- PR65初回のresponse.body()観測失敗の実encoding原因も未特定。実Downloadをprivate一時領域でmetadata/固定合成原本と照合しfinally削除する合格済み手順を保持する。元の期待値は緩和しない

### 最小統合と確認計画

1. Frozen [Domain/API](../specs/2026-10-02-organization-client-v0-domain-api-design.md)・[UI](../specs/2026-10-02-organization-client-v0-ui-design.md)、既存の[完了計画](../plans/2026-10-04-organization-complete-slice.md)・[保留再開計画](../plans/2026-10-04-organization-hold-resume-slice.md)を再利用する。新しい業務意味・認可方式を追加しない
2. mainのSearch/Document migration9/10/Outbox11・分割API・lock・workflowを保持する。Work独立schemaの追加migration5/6、PR65製品sourceを保持する。共通試験harnessのread診断と新操作段階を両立させる。active.mdの競合は両履歴を残して解消する。旧G9見出しだけはmainで整理済みの「Superseded checkpoint」を維持し、本文履歴は全て残す
3. ローカルでは純粋試験・型・schema・buildと限定差分レビューだけを行う。DB/socket/listener/browserを起動しない。新規依存や大きな検証基盤を追加しない
4. 日本語Draft公開後、同じ固定2名・一時DB・Chromium・画像/trace/video無しの通常CIを終端まで確認する。組合せのexact headで実DB/transaction・操作・再起動・復元・cleanupを区別して記録する

限定回帰: 2026-10-05 01:24 UTC、統合treeでGUI240件/20 suites、Organization純粋runner20件、application/runtimeの型、schema freshness、production buildが成功した。Webpackの既存advisory3件。Rust製品pathは受入元blobのまま、新しい組合せの全Rust/実DB資格はhosted通常CIで確認する。

現在のmain push CIは[このrun](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37250646282)を別途確認中。上記の受入元結果を新統合headの合格へ付け替えない。実LLM/本番Identity/Tauri実機/実サーバー反映の資格ではない。main mergeは親が直列調整し、導入は所有者の手動作業とする。

次のexact action: 最終差分の独立レビューと限定回帰を完了し、両parent保持の独立Draftを公開してexact-head全CI/実受入を確認する。

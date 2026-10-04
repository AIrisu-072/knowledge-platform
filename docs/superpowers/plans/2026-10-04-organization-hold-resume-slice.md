# Organization Browser PoC — 保留と再開の最小実装

目的: 現在担当中のタスクを人間が保留し、同じ試行・担当・保存内容のまま再開できるようにする。

基点は[PR63](https://github.com/AIrisu-072/knowledge-platform/pull/63) `cc994b4e81356f01beb8a81b8297c83741346926` / tree `826cba11636a76c84605a10369009b47926b3584`。独立branch `feat/organization-hold-resume-slice`。Frozen [Domain/API §3・4・5・13](../specs/2026-10-02-organization-client-v0-domain-api-design.md)と[UI §7](../specs/2026-10-02-organization-client-v0-ui-design.md)を実装し、新たな業務ルールや本番Identityを作らない。

## 固定する最小範囲

- 同じ模擬Human2名の現在担当だけ、定義actionとexpected attempt/revisionを確認してactive→held→activeへ進む。ready/completedや別担当から実行しない
- 保存済みdefinitionは不変。新規fixture versionだけに既定のhold/resumeを加え、旧DBを自動昇格しない。追加migrationが必要な場合も旧checksumを保持する
- 既存actions APIのclosed union、Work transaction/OCC/operation ledger/history/stagingを使う。同operation回復と競合/rollbackを維持する
- 同じattempt/assignment/private内容・過去snapshotを保持し、新担当・提出・自動保存は作らない。保留中は業務内容を変更せず、現在権限による閲覧と明示再開を行う
- Agentの古いcontextに対する出力を保留/再開後に採用しない。再開だけで再実行しない。実LLM/MCP・新外部送信は無い
- UIはserver capability/action IDを使い、確認取消・結果不明回復・切替時の隔離を既存実装へ追加する。未保存の入力はタブ内だけに保持し、保留で保存したと表示しない

## TDDと受入

1. Domain/HTTPと既存transaction試験へ、遷移・同attempt/assignment/保存値不変・現在責任/action/OCC・重複/rollback・Agent fenceのREDを追加し最小実装
2. 生成API/共通UIへ保留→readonly→再開、取消、未保存入力、競合/不明結果、denial/切替のpure回帰を追加
3. 同じ2名journey内に営業/事務の短い保留と再開を追加し、既存最終完了・過去履歴/private・両HTTP server再起動/cleanupを維持する。新runnerや画像公開を増やさない
4. 独立限定レビュー、日本語Draft、同じ一時DB/Chromiumのexact-head通常CI

ローカルDB/socket/listener/browserは起動しない。別PRのSearch/main統合変更はこの枝へ含めず、PR63のみを基点に保留/再開を追加する。

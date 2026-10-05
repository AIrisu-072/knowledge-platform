# Organization Browser PoC — 合成Agentの最小縦断実装計画

目的: 担当タスクで認可済み根拠を選び、合成Agentの構造化候補を保存して、既存の人間判断・明示提出へ接続する。

基点は受入[PR57](https://github.com/AIrisu-072/knowledge-platform/pull/57) `d383baccddd5081687b500f064f6fce195a24816`。別branch `feat/organization-agent-slice` に実装し、受入sourceを保持する。Frozen [Domain/API §9・13–14・17](../specs/2026-10-02-organization-client-v0-domain-api-design.md)、[Product/UX §9](../specs/2026-10-02-organization-client-v0-product-ux-design.md)、[UI §5](../specs/2026-10-02-organization-client-v0-ui-design.md)を再利用する。実装は既存Rust/Axum/PostgreSQL、React、生成Work client、Tokioだけを使う。

## 境界と固定判断

- Humanはsales-01/office-01の2名のまま。実executorはtrusted adapter内のorganization-synthetic/agent-01、Document providerは既存StaticPoCIdentityAdapterが返すpoc/poc-agent。3者を同一視せず、clientからprincipal/provider/authorを受け取らない
- requester現在権限とprovider現在権限を、既存Document Revision/History Applicationサービスで別々に確認する。executorは実行ID・task/attempt・責任・選択したexact Evidence refs・context revision・read-only allowlistに固定する。各provider操作前と開示前に再検証し、remote/provider操作中にWork row lockを保持しない。各selected sourceの参照確認の直前/直後にWork execution/cancel/contextを再確認し、内部のRevision/History readは各々既存Document認可を行う。後続sourceを取消後に開始せず、cross-provider atomic transactionは主張しない
- owned disposable fixtureの初期化だけにpoc/poc-agentのRead/ReadHistory grantを追加する。既存DBのpolicy差分を暗黙更新しない。既存MCP実装・profilesは変更しない
- executorは固定規則の模擬処理。原本本文を読まず、内容を理解・検証・推論したと示さない。UI/resultに「合成実行・本文分析なし・実LLM/MCP通信なし」を明記する。実Document認可とMCP wire未実証を区別する
- 入力は既存Evidenceのexact revision参照1–16件とUTF-8最大8KiBの目的。出力は短い固定説明と不確実性を持つimmutable Finding1件、その根拠参照、実行provenance。Evidence複製・GeneratedArtifact・SuggestedAction・永続chatは追加しない
- AgentはHumanDecision、claim/submit/return、Document mutationを実行しない。生成Findingのauthorとoptional originExecutionIdをtrusted内部経路で確定し、Human登録のauthorを事後に書き換えない。候補作成の共通検証を再利用する
- 各現在attemptのexecutionは最大16件、同時実行はtaskごと1件。旧attemptの未選択private execution/resultは非開示。提出するのは既存Finding/Evidence/Decision membershipだけで、Agent request本文や実行履歴を自動共有しない

## 保存・実行・取消

1. 凍結POST `/tasks/{id}/agent-executions` でoperation/OCC/current attempt/責任/選択refsを検証し、queued executionとoperation receipt、必要stagingを同じWork transactionへ保存する。同ID同payloadの再送は同execution、変更payloadは競合
2. accepted IDを返し、アプリが所有するTokioの単発taskでのみdispatchする。独立worker/queue/汎用job frameworkを作らない。GETやoperation recoveryはdispatchしない。runningへの遷移もWork fenceで保護する
3. bounded adapterをWork lock外で実行し、直後にexecution ID・status・attempt・責任・context revision・選択refs・freshnessを再確認する。Finding＋succeeded result＋stagingを一つのWork transactionで確定。取消やcontext変更後のlate outputは保存・開示しない
4. 凍結POST `/agent-executions/{id}/cancel` は未完了の今後の仕事を止める。完了済をcancelledへ書き換えない。cancel/finishは同じlock/fenceで順序を決め、確定したFindingを取消で消さない
5. provider拒否/出力検証失敗など副作用なしと確認できる失敗はfailed。DB commit outcomeが不明なら既存COMMIT_OUTCOME_UNKNOWNと同ID回復を保持。process再起動時は当該固定profileが所有するqueued/runningをoutcome_unknownへ記録し、自動再実行しない。正常shutdownは所有taskを停止・drainしてからpoolを閉じる
6. 凍結GET `/agent-executions/{id}` と `/result` は現在Work/private/source権限を再確認する。存在・本文のoracleやstale cacheの開示を許さない。request receiptは過去状態の記録、現在状態はGETの正本として区別する

## 実装単位と最小確認

- [x] Work backend: `work-domain/src/agent.rs` と既存lib/evidence、`work-application`、`work-repository-postgres`の既存aggregate/ledger/stagingを拡張。追加migration0004のみ使用し0001–0003/checksum・旧command digestを保持。凍結4HTTP操作、closed OpenAPI、生成Work型へ接続する
- [x] Adapter/composition: `organization-server`の既存Document source helper、fixed provider identity、bootstrap、単発合成executor/dispatchとmainのdrainを接続する。既存Document business logicを複製しない
- [x] UI: `TaskHomePage`の共通Agent moduleを接続。目的/選択根拠→依頼→状態/取消→構造化候補→既存「根拠」moduleの人間判断を表示する。task/identity境界で入力・queryを破棄し、unknownは同operationの回復を使う
- [x] 最小TDD: 別Human/private既知ID拒否、requesterのみ/providerのみの拒否、principal override拒否、AgentがHumanDecisionを作れない境界、cancel/finishとlate response、task/attempt/context変更、exact replay/digest変更、rollback、source revocation後のresult/recovery非開示、上限超過を既存suite内で確認する
- [ ] 既存ignored PostgreSQL試験と同2名journey/persistenceを延長。Agent生成Findingから人間判断/明示提出、未選択非開示、両HTTP server再起動後復元、cleanupを同じ一時DB/Chromium/画像無しで確認する。新runner/workflowを作らない
- [ ] 限定独立レビューとDocument回帰を行い、新Draftのexact headで通常CI・DSI・Sandboxを終端確認する

新しい実LLM・外部サービス・credential・本番identity/data・Tauri/native・merge/deployは含めない。ローカルDB/socket/listener/browserは実行せず、許可されたpure tests/compileと既存hosted検証だけを使う。MCP wireおよびAgent全体/Phase5全体の完了は主張しない。

実装上の具体的な選定と影響は[判断記録](../../decisions/2026-10-04-organization-synthetic-agent-poc.md)にまとめる。個々の案について所有者の具体的な事前承認があったとは主張しない。

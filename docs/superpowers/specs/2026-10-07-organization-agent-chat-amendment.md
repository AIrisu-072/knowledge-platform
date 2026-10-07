# Organization Client v0 — AgentExecutionの構造化結果・Agent Chat・executor adapter境界の実装追補（U4）

状態：実装前の追補。凍結済みの[Product/UX](2026-10-02-organization-client-v0-product-ux-design.md) §8・§9、
[Domain/API/Auth](2026-10-02-organization-client-v0-domain-api-design.md) §8・§9・§13、
[UI](2026-10-02-organization-client-v0-ui-design.md) §5・§8 の意味を変えず、U1（[複数担当](2026-10-07-organization-multi-principal-amendment.md)）・
U2（[文脈・注意・表示Profile](2026-10-07-organization-work-context-attention-amendment.md)）・
U3（[作業ファイル・Handoff](2026-10-07-organization-work-files-handoff-amendment.md)）の上に、
「AgentExecutionの構造化結果（GeneratedArtifact・SuggestedAction・根拠ごとの利用結果）」「現在のタスクに付くAgent Chat」
「固定の合成executorを保ったまま、実Agent/MCPへ差し替えられるexecutor adapter境界」を接続する最小の具体化だけを記録する。
新しい業務判断は §10 に分けて明示し、承認済みとは扱わない。

## 1. 基点と範囲

- 基点：U3統合後のmain
- 既存の合成Agent（依頼・取消・状態・結果、要求者／executor／providerの独立した認可、遅延応答の遮断、停止時の結果不明）、根拠・候補・人間判断、工程、担当判定、文脈・注意、作業ファイルは再実装しない
- 本単位で変えるのは次の3点だけ
  1. 成功した実行の結果に、GeneratedArtifact（非公開の下書き候補）・SuggestedAction（実行権限の無い型付き提案）・選択した根拠ごとの利用結果を加える（Domain §9「Result carries … GeneratedArtifact/SuggestedAction references and explicit partial/uncertain/provider errors」）
  2. 既存の合成Agentモジュールを、現在のタスクの依頼と結果を時系列に並べるAgent Chatにする。結果は業務記録（AgentExecution・Finding・GeneratedArtifact・SuggestedAction）で、Chat本文だけに業務結果を置かない
  3. executorを `AgentExecutorPort` の背後へ移し、固定の合成executorをその実装の一つにする。実Agent/MCPの接続は手順だけを残し、実装・選定はしない（§9）
- モデル・provider・資格情報の選定、実LLM/MCP通信、Agentによる工程操作・人間判断・Document変更、Chat本文の永続化・個人Memoryは含めない

## 2. executor adapter境界

- `work-application` に `AgentExecutorPort` を置く。入力はWorkが組み立てて認可済みの `AgentDispatchContext`（実行ID・要求者と現在の担当・executorとprovider binding・許可された読取りtool・選択した根拠・文脈版）と残り時間、出力は `AgentOutput`（§3）だけ
- executorは読取り専用。Work・Documentへ書かない。資格情報・物理パス・client由来のprincipalを受け取らない
- 呼出しはWorkのrow lockの外。1回の上限時間は20秒。超過・executorの失敗は既存の `failed`（`dependency_unavailable`）として記録し、自動再実行しない
- executorの出力はWorkのDomainが検証してから記録する（§3）。検証に失敗した出力は既存の `failed`（`invalid_output`）とし、一部だけを記録しない
- 要求者・executor・providerの認可（既存の `AgentSourcePort` による選択した根拠ごとの再確認）、実行文脈の版の照合、遅延応答の遮断は変えない。記録の直前に従来どおり全ての選択した根拠を再確認し、いずれかが拒否なら記録しない
- 実行の識別（`executedBy=organization-synthetic/agent-01`、provider binding `document/poc/poc-agent`、`simulated` などの表示）はDomainの固定の許可一覧のままとする。新しいexecutorは、仕様の承認を経て許可一覧へ加えるまで使えない（§9）

## 3. 構造化結果（AgentOutput → AgentResult）

executorは次を返す。

| 項目 | 内容・上限 |
|---|---|
| summary | 1〜8192 bytes |
| uncertainty | 1〜4件、各1〜8192 bytes。不確実な点は必ず明示する |
| sourceOutcomes | 選択した根拠1件ごとに1件、同じ順序。`referenced`（参照情報だけを使用）・`analyzed`（本文を分析）・`unavailable`（取得できなかった）・`unsupported`（扱えない形式） |
| finding | 0〜1件。claimと根拠参照（`referenced`/`analyzed` の根拠の部分集合、1件以上）。根拠の無い提案はFindingにしない |
| generatedArtifacts | 0〜2件。題名（1〜200 bytes、改行・制御文字なし）と本文（既存の文案と同じ1〜8192 bytes）、出典の根拠参照（利用できた根拠の部分集合、1件以上） |
| suggestedActions | 0〜4件。型付きの提案と理由（1〜1024 bytes）。種類は閉じた語彙：`review_finding`（このfindingを確認して人間判断を記録する）・`use_generated_artifact`（この下書きを作業文案に使う） |

- 利用できた根拠（`referenced`/`analyzed`）が1件も無い出力は記録しない（`invalid_output`）
- 合成executorは本文を分析しないため `analyzed` を返せない（Domainで拒否）
- 一部の根拠が `unavailable`/`unsupported` の結果は「一部の根拠だけで作成した結果」であり、画面と応答で明示する。全体の検証済みとは表示しない
- 記録される `AgentResult` に次を追加する（いずれも空なら出力しない。U4以前の結果のJSONは不変）
  - `sourceOutcomes`、`generatedArtifactIds`、`suggestedActionIds`
- `findingRevisionRefs` は0〜1件になる（U4以前の結果は従来どおり1件）。`evidenceRevisionRefs` は従来どおり選択した根拠
- 合成executorの出力（固定）：既存のsummary・候補claim・不確実な点、全根拠 `referenced`、下書き1件（選択した根拠の一覧と「本文未分析・採否は人間が判断」の注記だけを含む固定文。依頼目的・原本本文は含めない）、提案2件（候補の確認、下書きの利用）

## 4. GeneratedArtifact（非公開の下書き候補）

- `{id, executionId, contextId, workItemId, attemptId, schemaId, title, value, sourceRevisionRefs, author, simulated, visibility, createdAt}`。`schemaId` は既存の文案と同じ `organization.text-draft.v1`、`visibility` は `agent_execution_private`、`author` はexecutorの識別
- 閲覧できるのは、その実行を閲覧できる者だけ（既存のAgentExecutionと同じ：要求者本人、同じ試行、同じ現在の担当、タスクの閲覧権限）。読むたびに選択した根拠の要求者・providerの認可を再確認する。担当変更・取消・試行の変更の後は読めない
- 下書き候補はWorkの作業成果物ではない。提出・Handoff Snapshotの対象にならず、次工程から見えない
- 作業へ使うのは既存の通常経路だけ：担当者が画面で「作業文案に入れる」を選ぶと、候補を現在の権限で読み直して作業中の文案（未保存）へ入れ、担当者が内容を確認して既存の「文案を保存」で保存する。保存した文案は担当者の作業成果物であり、候補への参照は持たない
- 保存前の文案に未保存の変更がある間、編集権限が無い工程、複数の文案がある場合は入れられない（理由を表示する）

## 5. SuggestedAction（実行権限の無い型付き提案）

- `{id, executionId, contextId, workItemId, attemptId, action, rationale, supportingRevisionRefs, author, visibility, createdAt}`。`action` は §3 の閉じた語彙で、対象（finding版・下書きID）を持つ
- 閲覧規則はGeneratedArtifactと同じ
- 提案を実行するAPIは無い。人間が選ぶと、画面は提案と対象を現在の権限で読み直し、通常の画面（根拠・判断モジュール、作業文案）を開くだけ。人間判断の記録・文案の保存は既存の操作が改めて認可する
- 提案はHumanDecisionではなく、工程操作でもない

## 6. Agent Chat（現在のタスクに付くContext Surface）

- 主ナビゲーションの項目・第3の画面型にはしない。営業型・事務型のどちらでも、タスク詳細のContext Surfaceの「Agent」モジュールとして表示する（既存の位置・名前を保つ）
- 表示：このタスクの現在の試行で本人が依頼した実行を時系列に並べる（最大16件）。選んだ1件を展開し、依頼（目的・選択した根拠）と応答（状態・要約・根拠ごとの利用結果・不確実な点・候補・下書き・提案）を並べる。他の実行は折りたたみ、開いたときだけ読む
- 入力欄（依頼目的・根拠の選択）は既存と同じ制約。送信は既存の `POST /tasks/{id}/agent-executions`
- Chatの表示は業務記録の投影であり、Chat用の記録は増やさない。入力中の内容・画面内の通知はこのタブの中だけで保持し、利用者・担当・タスク・試行が変わると消える（既存の非公開入力と同じ）
- 識別の区別（要求者・担当・実際のexecutor・provider principal）、模擬処理の表示（固定規則・本文未分析・実LLM/MCP通信なし）は従来どおり表示する

## 7. API

| Method / path | 内容 |
|---|---|
| GET `/agent-executions/{id}/result` | 既存。§3の項目を追加（空なら省略） |
| GET `/generated-artifacts/{id}` | 設計§13の表にある `getGeneratedArtifact`。非公開の下書き候補を現在の権限で返す |
| GET `/suggested-actions/{id}` | 設計§13の表にある `getSuggestedAction`。実行権限の無い型付き提案を返す |

- 閲覧できない・存在しない場合は、既存のAgentExecutionと同じ `404 WORK_ITEM_NOT_FOUND`（存在を区別しない）
- 新しい変更APIは無い

## 8. Audit担当へのhandoff（共通schemaは変更しない）

- stagingの種類は増やさない（migration無し）
- `agent_execution_succeeded` のpayloadに、空でない場合だけ `generatedArtifactIds`・`suggestedActionIds`・`sourceOutcomes`（根拠ID・版・利用結果）を追加する。題名・本文・理由・依頼目的は入れない

## 9. 実Agent/MCPへの接続手順（依頼者向け、本単位では実施しない）

1. 依頼者がモデル・provider・資格情報の管理方式・実行環境を選定し、仕様（Domain §9の識別・認可の義務）を承認する
2. `AgentExecutorPort` の新しい実装を、読取り専用の既存Document MCP（provider `poc`・principal `poc-agent` の起動時確認は不変）などに接続して作る。要求者の現在の権限とprovider principalの権限の両方を毎回確認できないproviderは使えない
3. Domainの許可一覧（executorの識別・provider binding・`simulated` 等の表示）へ新しいexecutorを加える変更を、独立reviewと受入を経て行う
4. 組立て（`organization-server` の起動処理）で合成executorの代わりに渡す。合成executorは試験用に残す
- 本番接続・実データ・サーバー反映は本単位で行わない

## 10. 新しい判断（承認済みとは扱わない）

1. 根拠ごとの利用結果を `referenced`・`analyzed`・`unavailable`・`unsupported` の4種とし、一部の根拠だけで作った結果を成功（一部）として記録する
2. 結果のFindingを0〜1件とする（根拠の無い提案はFindingにしない）
3. GeneratedArtifactは1実行につき最大2件、文案と同じtext形式だけ。題名は200 bytes以内
4. SuggestedActionは1実行につき最大4件、種類は `review_finding`・`use_generated_artifact` の2つだけ（工程操作の提案は含めない）
5. 下書き候補の採用は「作業中の文案（未保存）へ入れる→担当者が既存の保存操作で保存」だけとし、保存した文案に候補への参照を残さない
6. executor 1回の上限時間を20秒、超過は `failed`（`dependency_unavailable`）とする（読取り専用のため外部の副作用は無い前提）
7. Agent Chatは選んだ1件だけを展開し、他は折りたたむ（開いたときだけ読む）
8. GeneratedArtifact・SuggestedActionの閲覧不可は `WORK_ITEM_NOT_FOUND` で返す

## 11. 範囲外・残件

- 実Agent/MCP・実LLMの接続、モデル・provider・資格情報の選定（§9の手順のみ）
- Agentへの文脈の追加（作業文案・受領した提出・業務履歴を選んで渡すこと）。本単位の選択対象は既存どおり根拠だけ
- 工程操作（提出・差戻・完了）の提案、Document・Searchへの操作の提案
- 下書き候補のDocumentへの昇格、ファイル形式の下書き
- Chatの自由対話（複数往復の文脈保持）・Chat本文の保存・個人Memory

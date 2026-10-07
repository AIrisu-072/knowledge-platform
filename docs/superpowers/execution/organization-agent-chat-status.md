# Organization Agentの構造化結果・Agent Chat・executor adapter境界（U4）— Capability Execution Status

## 2026-10-07 — 実装・ローカル検証中

### 位置付け

- U3（作業ファイル・共有provider・Handoff・差戻し後の作業、[PR #102](https://github.com/AIrisu-072/knowledge-platform/pull/102)、main `3b06421`）の上に、凍結設計のAgentExecutionの構造化結果（GeneratedArtifact・SuggestedAction・根拠ごとの利用結果）、現在のタスクに付くAgent Chat、固定の合成executorを保ったexecutor adapter境界を接続する
- 設計の具体化：[実装追補](../specs/2026-10-07-organization-agent-chat-amendment.md)、手順：[小計画](../plans/2026-10-07-organization-agent-chat.md)、利用手順：[Organization Browser PoC](../../operations/organization-browser-poc.md#agent-chatで構造化結果を確認し提案から通常の操作へ進む)
- 既存の合成Agent（依頼・取消・状態・結果、要求者／executor／providerの独立した認可、遅延応答の遮断、停止時の結果不明）、根拠・候補・人間判断、工程、担当判定、文脈・注意、作業ファイルは再実装しない
- branch：`claude/trusting-knuth-dn5cx4`（U3統合後のmain `3b06421` から作り直し、ローカルで作ったU4 commitを移した）

### 実装した内容

- Domain（`work-domain`）：`AgentOutput`（executorの提案）の検証（選択した根拠ごとの利用結果を同順で1件ずつ、利用できた根拠だけを引用、合成executorは本文分析を主張できない、上限）、`GeneratedArtifact`（非公開の下書き候補）・`SuggestedAction`（`review_finding`・`use_generated_artifact` の閉じた語彙、実行権限なし）の記録・閲覧・整合性。閲覧は既存の実行の閲覧規則（依頼者本人・同じ担当・同じ試行・タスクの閲覧権限、成功した実行だけ）。`AgentResult` に `sourceOutcomes`・`generatedArtifactIds`・`suggestedActionIds`（空なら省略）、Findingは0〜1件。U4以前の結果・保存JSON・操作digestは不変
- Application：`AgentExecutorPort`（認可済みの実行文脈だけを受け、読取り専用、1回20秒）
- Repository：候補の読取り（domainの範囲確認→要求者・providerの再確認→同一内容の再読込）。stagingは `agent_execution_succeeded` に参照（ID・利用結果）だけを追加し、題名・本文・理由・依頼目的は入れない。migration無し
- Server：合成executorを `SyntheticAgentExecutor`（portの実装）へ移し、組立てで明示的に渡す。executorの上限時間・失敗・不正な出力は `failed`（`dependency_unavailable`／`invalid_output`）、自動再実行しない
- HTTP/OpenAPI：`GET /generated-artifacts/{id}`・`GET /suggested-actions/{id}`（設計§13の表にあるもの）。閲覧不可は `404 WORK_ITEM_NOT_FOUND`。実行APIは無い。生成型を再生成
- GUI：既存の「合成Agent」モジュールをAgent Chatにした（依頼と結果の時系列、最新の1件を展開・他は折りたたみ、根拠ごとの利用結果、一部の根拠の明示、下書き候補・提案）。下書き候補は現在の権限で読み直してから未保存の作業文案へ入れるだけ（保存は既存の文案保存。未保存の変更・編集不可の工程・複数文案では入れない）。提案は読み直して根拠・判断モジュールまたは作業文案へ進むだけ
- 受入：作業ファイルのpersistenceの後、同じDBで agent-chat-journey → 6 process再起動 → agent-chat-persistence

### 検証（ローカル）

| 区分 | 結果 |
|---|---|
| Domain | 全pass。新規 `tests/agent_chat.rs` 4件（構造化結果の記録と閲覧・JSON互換、一部の根拠と出力の上限・閉じた提案語彙・Findingの無い結果、担当変更後の非開示と整合性の改ざん検出、差戻し後も完了した事務の試行が自分の記録を保持）。変異確認：出力検証と整合性検証の二重の層（利用できた根拠だけの引用・合成executorの本文分析・利用できる根拠なし）は両方外すとRED、未掲載の候補・提案の対象・重複した提案はそれぞれ外すとRED |
| 実PostgreSQL 16 | 4件pass（候補の保存と閲覧、他の担当からの非開示、要求者・providerどちらの拒否でも候補を返さない、stagingは参照だけで題名・本文・理由を含まない） |
| Server（executor port） | 7件pass（新規：上限時間・executorの失敗・不正な出力を `failed` として記録し再実行しない、合成出力は依頼目的・該当箇所を含まない） |
| HTTP | 全pass（新規：候補・提案の取得、他の担当と未知IDは同じ404、書込みmethodは閉じた `VALIDATION_FAILED` で状態不変） |
| GUI | 1642/1642（69 suites）、型検査（GUI・Organization runtime）。新規：Agent Chat 7件・client 1件。変異確認：候補の読み直し・未保存の変更の保護・提案の対象照合・一部の根拠の表示・編集不可の工程の保護をそれぞれ外すとRED |
| 静的検査 | fmt、clippy（変更crate、`-D warnings`）、architecture-lint、assurance（scan/plan/run）、repo policy、API contract 18件、organization OpenAPI lint（警告0）、runner node試験30件 |
| 実browser（ローカル、PostgreSQL 18.6公式image、system Chromium） | 23 stageすべてpassed（既存2名・6名policy・文脈・作業ファイルに加え agent-chat-journey／agent-chat-restart／agent-chat-persistence、cleanup完了）。既存2名journeyは新しいAgent Chat画面のまま変更なしで通過 |

### 新しい判断（承認済みとは扱わない）

[実装追補§10](../specs/2026-10-07-organization-agent-chat-amendment.md#10-新しい判断承認済みとは扱わない)の8点。

### 受入済みjourneyへの変更

- `support.ts` の1行：`findingRevisionRefs` の生成型が1件のtupleから配列（0〜1件）になったため、既存の確認に型注記（`!`）を加えた。動作は不変

### Audit担当へのhandoff（共通schemaは変更していない）

- `agent_execution_succeeded` のpayloadに、空でない場合だけ `generatedArtifactIds`・`suggestedActionIds`・`sourceOutcomes` を追加。題名・本文・理由・依頼目的は入れない

### 次のexact action

1. ローカル受入（23 stage）・全GUI試験の結果を記入
2. 独立reviewの指摘を修正
3. PR作成（Draft）・PR activity購読・exact-head CI → main統合 → 統合後CI

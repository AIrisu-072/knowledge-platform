# Organization Agentの構造化結果・Agent Chat・executor adapter境界（U4）— Capability Execution Status

## 2026-10-07 — 実装・ローカル検証・独立review修正完了、PR/hosted CIへ

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
| Server（executor port） | 7件pass（新規：上限時間・executorの失敗（結果不明・拒否を名乗る場合を含む）・不正な出力を `failed` として記録し再実行しない、合成出力は依頼目的・該当箇所を含まない） |
| HTTP | 全pass（新規：候補・提案の取得、他の担当と未知IDは同じ404、書込みmethodは閉じた `VALIDATION_FAILED` で状態不変） |
| GUI | 1646/1646（69 suites、review修正後）、型検査（GUI・Organization runtime）。新規：Agent Chat 11件・client 1件。変異確認：候補の読み直し・未保存の変更の保護・提案の対象照合・一部の根拠の表示・編集不可の工程の保護をそれぞれ外すとRED |
| 静的検査 | fmt、clippy（変更crate、`-D warnings`）、architecture-lint、assurance（scan/plan/run）、repo policy、API contract 18件、organization OpenAPI lint（警告0）、runner node試験30件 |
| 実browser（ローカル、PostgreSQL 18.6公式image、system Chromium） | review修正前・修正後とも23 stageすべてpassed（既存2名・6名policy・文脈・作業ファイルに加え agent-chat-journey／agent-chat-restart／agent-chat-persistence、cleanup完了）。既存2名journeyは新しいAgent Chat画面のまま変更なしで通過 |

### 独立review（GO）と対応

重大な指摘は無し（GO）。Important 1件と軽微事項を次のとおり対応した（各修正は外すと試験がRED）。

| 指摘 | 対応 |
|---|---|
| Important：executorが返した誤りの種類（結果不明・拒否など）がそのまま記録され、adapterがWorkの確定を不明と主張できる | executorの誤りは種類を問わず `dependency_unavailable` として記録（追補§2・§10-10）。結果不明・拒否を返すexecutorの試験を追加 |
| 下書き候補の読み直し中に編集権限を失っても、戻ったときに文案へ入る | 戻った時点の編集可否（権限・文案数・処理中）で判断し、入れない場合は通知 |
| 読み直しがIDだけを照合し、`review_finding` の対象を読み直さない | 下書き候補・提案は表示中の実行・タスク・試行と一致しなければ非表示扱い。候補（Finding）を読み直し、版・試行・生成元の実行を照合 |
| 一時的な `409 WORK_CONTEXT_STALE` で候補が誤り表示のまま残る | 実行の読取りと同じ規則で再読込、「実行状態を再読込」で候補も読み直す |
| 受入の `toHaveCount(0)` が空振りし得る | 入力欄と根拠の表示を先に確認 |
| 追補に無い判断（置き換え、記録の保持、summaryの文言） | 追補§10に置き換えを追記、§9の手順と§11に保持期間・NO_RETENTION確認を追記、§3のsummaryの記述を修正 |
| OpenAPIの `maxLength` は文字数で、上限はbyte | 説明にbyteと明記済み（serverとdecoderはbyteで検査）。変更なし |

### 新しい判断（承認済みとは扱わない）

[実装追補§10](../specs/2026-10-07-organization-agent-chat-amendment.md#10-新しい判断承認済みとは扱わない)の10点（独立reviewの指摘で2点を追記）。

### 受入済みjourneyへの変更

- `support.ts` の1行：`findingRevisionRefs` の生成型が1件のtupleから配列（0〜1件）になったため、既存の確認に型注記（`!`）を加えた。動作は不変

### Audit担当へのhandoff（共通schemaは変更していない）

- `agent_execution_succeeded` のpayloadに、空でない場合だけ `generatedArtifactIds`・`suggestedActionIds`・`sourceOutcomes` を追加。題名・本文・理由・依頼目的は入れない

### 次のexact action

1. [PR #104](https://github.com/AIrisu-072/knowledge-platform/pull/104) のexact-head CI（review修正後のhead）を確認
2. 合格後mainへ統合し、main push CIを確認

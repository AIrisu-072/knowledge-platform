# Organization Agentの構造化結果・Agent Chat・executor adapter境界（U4）— 小計画

設計：[実装追補](../specs/2026-10-07-organization-agent-chat-amendment.md)。基点：U3統合後のmain。TDD（RED→GREEN→変異確認）、合成データのみ。

## Task 1 — Domain（`work-domain`）

1. 試験 `tests/agent_chat.rs`（RED）：構造化結果の記録と閲覧、一部の根拠の明示と利用できた根拠だけの引用、出力の上限と閉じた提案語彙、Findingの無い結果、担当変更・差戻し後の閲覧、U4以前の結果の互換
2. `agent_result.rs`：`AgentOutput`（executorの提案）の検証、`GeneratedArtifact`・`SuggestedAction` の記録・閲覧・整合性
3. `AgentResult` に `sourceOutcomes`・`generatedArtifactIds`・`suggestedActionIds`（空なら省略）。Findingは0〜1件

## Task 2 — Application・Repository・Server・HTTP

1. `AgentExecutorPort`（認可済み文脈だけ、読取り専用、20秒上限）
2. Repository：候補の読取り（providerの再確認→同一内容の再読込）、stagingは参照だけ（migration無し）。実PostgreSQL試験
3. Server：合成executorをportの実装へ移し、組立てで明示的に渡す。上限時間・executorの失敗・不正な出力の試験
4. HTTP/OpenAPI：`GET /generated-artifacts/{id}`・`GET /suggested-actions/{id}`（設計§13の表にあるもの）。実行APIは作らない

## Task 3 — GUI

1. decoder（構造化結果・下書き候補・提案）とAPI client
2. Agent Chat：時系列の依頼と結果、根拠ごとの利用結果、下書き候補（読み直してから未保存の作業文案へ）、提案（読み直して通常の画面へ）
3. 試験（変異確認を含む）

## Task 4 — 実browser受入と文書

1. `agent-chat-journey.spec.ts`・`agent-chat-persistence.spec.ts`、runnerのstage（agent-chat-journey → agent-chat-restart → agent-chat-persistence）と閉じた診断語彙
2. 利用手順（日本語、実Agent/MCPの接続手順を含む）、状況、active pointer
3. 独立review → 修正 → PR → exact-head CI → main統合 → 統合後CI

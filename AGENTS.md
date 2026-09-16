# Agent entrypoint

このrepositoryでは `spec/` がnormative SSOTです。

作業開始時に全文を読み込まず、対象変更に関係する仕様だけを参照してください。
最終的な制約判定はDevelopment Assurance / CIに従います。

基本コマンド:

- `mise run verify:fast`
- `mise run verify`
- `mise run verify:full`

重要:
- 顧客固有名詞・実データ・秘密情報をrepositoryへ入れない。
- `POC REQUIRED` dependencyをproductionへ追加しない。
- production実装を始める前に該当specとimplementation planを確認する。

## Active execution / session resume

`docs/superpowers/execution/active.md` が存在し、Statusが `ACTIVE` の場合は、会話履歴や記憶から進捗を再構成しない。

新しいsession/agentは次の順に読む:

1. `AGENTS.md`
2. `docs/superpowers/execution/active.md`
3. Active Pointerが指定するCapability Execution Status
4. Active Pointerが指定するApproved Design Spec
5. Active Pointerが指定するImplementation Plan
6. Statusが指定するGitHub branch / PR / CIの**現在状態**

repositoryとGitHubの現在状態がchat/記憶より常に優先される。

作業sessionを終了・切替する前、またはcontext上限が近い場合はCapability Execution Statusを更新し、最低限以下を残す:

- 完了したTask / Step
- 現在のTask / Step
- 実行したverificationと結果
- 使用branch / PR
- blocker / 未解決判断
- 次に行う**exact action**
- Design Freezeからの差分提案と承認状態

freshなverification evidenceなしに完了を宣言しない。

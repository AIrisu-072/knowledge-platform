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

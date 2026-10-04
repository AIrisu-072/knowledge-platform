<a id="p6-postgresql-policy-row-lock-role-ruling"></a>
# P6 PostgreSQL ポリシー行ロックに必要なロール権限の判断

[固定された公開原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-policy-lock-role-ruling.md)に対応する意味保存の日本語訳です。原設計の再承認、実装・資格の追加ではありません。既存ハッシュは当時の原文・証拠を指し、訳文のハッシュではありません。以下の状態・次の作業は当時の記録であり、現在の実行指示ではありません。[最新の実行状態](../../execution/search-platform-completion-program-status.md)を優先してください。

PostgreSQLのSELECTロック句では、ロック対象の各テーブルについて、少なくとも1列に対するUPDATE権限が必要です。G02は凍結済みの `FOR SHARE` ポリシーロックを維持します。配送ロールには、ポリシーのSELECT権限と、列を限定した `UPDATE(policy_id)` だけを付与します。policy_idはNOT NULL / PRIMARY KEYで、CHECK(policy_id=1)があるため、許されるのは値を変えない代入だけです。改訂番号、試行回数、リース、バックオフのポリシーは変更できません。

INSERT、DELETE、TRUNCATEと、それ以外のポリシー列のUPDATEは引き続き拒否します。値を変えないこの権限から、ポリシー上のトリガーが追加の副作用を生じさせる変更には、新たな適格性検証が必要です。G08/I04は実際のロールで、ロックの成功、各ポリシー値変更の拒否、key2/nullに対するCHECK制約の拒否、INSERT/DELETE不可を証明しなければなりません。起動時・claim/reap内の期待ポリシー一致条件は変更しません。

これは既存の意味を最小権限で実現するための技術判断であり、ポリシー設定を編集する権限ではありません。

一次資料：https://www.postgresql.org/docs/current/sql-select.html のロック句に関する権限規則。実ロールの回帰検証では、固定したPostgreSQL18フィクスチャを基準とします。

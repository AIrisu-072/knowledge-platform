<a id="p6-generic-durable-outbox--design-freeze"></a>
# P6 汎用永続 Outbox の設計凍結

> 以下の状態・次の作業は当時の記録であり、現在の実行指示ではありません。現行の停止条件・実行可否は[最新の実行状態](../../execution/search-platform-completion-program-status.md)を参照してください。

[公開原文の固定リンク（commit `0ecf486719e3c9d71242e289a7564ad6d1032b3c`）](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-freeze.md)

本書は意味保存の日本語訳であり、原設計の再承認・実装/資格の追加ではない。既存ハッシュは当時の原文/証拠のものであり、訳文のハッシュではない。以下の状態・予定・合否は当時の記録として保持する。本文の行番号は同 commit の各原文を指し、従来の見出しアンカーも維持している。掲載コマンドは今回の翻訳作業では実行していない。

用語: Outbox は配送待ちイベントの永続表、Source は検索対象の情報源。リースは期限付き処理権、フェンスは古い所有者の更新を拒否する条件、エポックは所有権の世代番号、Search のイベント処理記録（receipt）は索引処理結果の記録、検証の receipt は試験・実行結果の証拠記録を指す。CAS は期待する現値との一致を条件にした更新、GC は不要世代の回収、DLQ は打切りイベントの保管先。Search / Domain / Document / Folder / AccessPolicy / Projection / Unit / Graph などの構成名・型名は識別のため維持する。

- 状態: **FROZEN**。アーキテクチャ契約と範囲を限定した実装計画を凍結済み。実装や DB による資格確認の完了を主張するものではない。
- 権限の根拠: Completion Program（完成プログラム）は、自律的な凍結と実装を明示的に許可している。
- 正確な設計: [承認対象の固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-design-revision-1.md)、SHA-256 `c886a6b0933d6323b0fab7d41f0641e18332c6c74104025442a03609369d5366`。[読者向け日本語訳](p6-outbox-design-revision-1.md)は別の文書バイト列である。
- 独立した GO 判定: [承認対象の固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-architecture-recheck.md)、SHA-256 `9728fffa22b501ada2f408735a3a325b592a2ba63b4d808e7ac800a5649be88c`。元の P2 指摘四件は閉鎖し、新しい P1/P2 はない。[読者向け日本語訳](p6-outbox-architecture-recheck.md)も参照できる。
- 維持する境界: 汎用配送処理が delivered_at を所有し、Search コンシューマーがグラフ・射影の公開を所有する。リースとフェンスで配送実行、条件付き処理記録・公開を検証する。Source フェンスは分散環境で機能するもので、プロセス内の相互排他ロックではない。
- 計画: [承認対象の固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-plan.md)、SHA-256 `29638c077a29aebd007be4a4fd7c5fb7f7bfbf4a785ba0540782f98795811a80`。範囲を限定した 21 タスクで、汎用基盤、Search 接続処理、共有の追加型マイグレーションを分担する。[読者向け日本語訳](p6-outbox-plan.md)も参照できる。
- 実行: 汎用部分の検証記録と全体統合の検証記録は分ける。資格確認の依存が循環しないよう、最終的な P7 の組み立てより先に共有永続基盤を実装する。
- 未達: TDD、実 PostgreSQL の並行・障害試験、独立したコードレビュー、P7 統合、正確な head に対するホステッド受け入れ確認。

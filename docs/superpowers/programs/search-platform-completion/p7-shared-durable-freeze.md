<a id="p7-shared-durable-composed-design-freeze"></a>
# P7 共有永続化基盤の統合設計凍結記録

[翻訳元の固定原文（commit 0ecf486719e3c9d71242e289a7564ad6d1032b3c）](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-shared-durable-freeze.md)

本書は意味保存の日本語訳であり、原設計の再承認・資格追加ではありません。既存承認hashは当時の原文・証拠を指し、訳文hashではありません。以下の状態・次の作業は当時の記録で、現在の実行指示ではありません。本文中の元の行番号も当時の原文を参照しており、訳文の行番号ではありません。旧見出しへのリンクは明示的なIDで維持しています。

改訂後の独立レビューでGOを得たため、親担当が自律的に設計を凍結しました。五つの指摘に関わる範囲では、改訂1が元の草案に優先します。P1/P3/P6で凍結した意味と、P5のSource種別に依存しない適用範囲は、すべて維持します。本番SQL、ロール、障害時の挙動、復元は、それぞれ別の受入ゲートです。

以下のSHA-256は当時の承認対象を表します。原文へのリンクを固定し、現在の文書を別に案内しています。

- [p7-shared-durable-design.mdの固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-shared-durable-design.md) SHA-256 `99a989e30ec77ea5f79915a3db22ecd31b910decbabc825c2ae8bca6f1041a3b`（[現在の日本語版](p7-shared-durable-design.md)）
- [p7-shared-durable-review.mdの固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-shared-durable-review.md) SHA-256 `2445530eb39254869dae75792c200108ad18fde28d2628685307af8b3ac836be`（[現在の日本語版](p7-shared-durable-review.md)）
- [p7-shared-durable-revision-1.mdの固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-shared-durable-revision-1.md) SHA-256 `2e2f1a24972f2020278cfe7fe37c1726ae280e936685ef74299c1c5c2a8641fe`（[現在の日本語版](p7-shared-durable-revision-1.md)）
- [p7-shared-durable-recheck.mdの固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-shared-durable-recheck.md) SHA-256 `f020a708e16aeb3e14d32a4dc73fca147f2c58f0f7752922459b893073bdf248`（[現在の日本語版](p7-shared-durable-recheck.md)）
- [p5-api-contract-revision-2.mdの固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p5-api-contract-revision-2.md) SHA-256 `81c8be90375bd2461fa8179ef7d5fdb0705c1b02431dee918d99ac26372901d9`（[現在の文書](p5-api-contract-revision-2.md)）

P6-I03がSearch0001と独立したSQLx移行台帳を所有します。その後に追加する所有権・完全世代・pin（世代の固定）・guard（構築中の保護）・GC（不要な世代の回収）のスキーマは、0002から始めます。P3の実バックエンドの資格判定と、厳密なGraphReceiptMappingV1エンコーダーの判定がGOになるまで、Graphの本番スキーマ、READY（準備完了）、公開には進めません。Graphに依存しない純粋な共有基盤は、範囲を限定した計画に従って進められますが、公開は閉じたままです。独立した第二のSourceポインターや、二つ目のactor/Source発行処理は作りません。イベント用の準備済み候補には、変更不能なイベントとエポックの結び付けが必須です。また、完全構築の対象を守るguardのトークンとフェンスも必須です。Sourceの種別と所有テナントは、削除済みを示すtombstoneになった後も変更できません。

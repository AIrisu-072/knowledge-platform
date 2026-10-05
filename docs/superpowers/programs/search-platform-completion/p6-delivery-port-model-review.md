<a id="p6-g01--p6-s01-typed-portmodel-独立レビュー"></a>
# P6-G01 / P6-S01 型付きポート・モデルの独立レビュー

[固定された公開原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-delivery-port-model-review.md)に対応する意味保存の日本語訳です。原設計の再承認、実装・実行時の適格性検証の追加ではありません。既存ハッシュと実行結果は当時の原文・証拠を指し、訳文のハッシュや現在の検証結果ではありません。以下の状態と次の作業は当時の記録です。[最新の実行状態](../../execution/search-platform-completion-program-status.md)を優先してください。

## 判定と範囲

- **G01: 条件付き GO。** 汎用モデル、許可リスト内のエラー、上限付き設定、型付き`OutboxStore` は凍結計画の境界に一致する。ただし下記 P3 の再試行のテストベクトルを G05 ランナー接続前に確定する。
- **S01: 静的 API 契約は GO、対象限定テストは保留。** Search 側に SQLx 型を渡さず、二つのフェンス、期待する現在値のキー・二つのダイジェスト・改訂番号、条件付き完了処理ポートを表現している。現在の `search-application` ビルド障害は S01 の編集範囲外であり、S01 の動的検証のGREEN 検証記録には使えない。
- 本判定は **ポート・モデルのみ**。DB 処理権取得/配送結果確定、Search完了トランザクション、P7 永続READY、汎用側の配送成功確定、統合資格の成功は判定していない。P6-N01 は規範改訂の検証記録であり、実装検証記録ではない。

照合した正本は `p6-outbox-design-revision-1.md` SHA-256 `c886a6b0933d6323b0fab7d41f0641e18332c6c74104025442a03609369d5366`、21タスクの `p6-outbox-plan.md` SHA-256 `29638c077a29aebd007be4a4fd7c5fb7f7bfbf4a785ba0540782f98795811a80`（G01:90–93、S01:162–165）。対象ブランチは `feat/search-platform-completion-core`、観測時 HEAD は `80a47960d025e4dfdea1eacade28b15d218725ff`、対象実装は未コミット作業木にある。

## 指摘

<a id="p3--retry-jitter-の-exact-vector-が凍結計画と一致するか未確定"></a>
### P3 — 再試行待機の揺らぎの厳密なテストベクトルが凍結計画と一致するか未確定

`p6-outbox-plan.md:90` の式は `SHA-256(event_id || attempt) mod (base/4+1)` だが、`crates/outbox-delivery/src/policy.rs:97–104` はダイジェストの**先頭 8バイトだけ**を `u64` として剰余を取る。例えば単体テストのイベントID `21213141-7271-8281-8284-590452353602` と試行回数 1、試行回数を符号付きi32のビッグエンディアンと解釈すると、実装の待機時間の揺らぎは 149 ms、256ビットのダイジェスト全体の剰余は 55 ms。`policy.rs:179–196` は範囲・単調性・安定性を確認するが、この違いを検知しない。

配送の安全性を破る差ではないが、凍結式を厳密な契約とするなら先頭 8バイト解釈と試行回数のバイト表現を計画に明記して承認するか、実装を式に合わせ、固定テストベクトルによるテストを追加する。G02 の DB実装担当は独立に進められる。G05 の再試行待機接続・資格付け前に解決する。

## 確認できた契約

- `crates/outbox-delivery/src/model.rs:12–140` はペイロードを含む配送情報、処理権取得試行回数・上限/トークン/所有者/期限、`Updated|Lost`、`Applied|KnownNoop|Retryable|Terminal`、7個の固定エラーコード、`PolicyMismatch|LegacyExhausted|InvalidConfig|StoreUnknown`、全 `OutboxStore` シグネチャを計画どおり公開する。DB エラー/コミット結果不明と確認済み 0 行 `Lost` の区別もトレイト文書にある。汎用クレートは Search をインポートしない。`DeliveryEnvelope` の `Debug` はペイロードを含むが、この範囲にペイロードを記録する観測・ログ処理は存在せず、現時点の漏えい指摘にはしない。
- `crates/outbox-delivery/src/policy.rs:10–83` はポリシー改訂番号と DB 初期値相当の試行回数8 / 1–120 s リース / 1–300 s 再試行待機、設定のバッチ `1..=32`・処理中 `1..=8`・期限切れ処理の回収 `1..=32`・`renew_interval < lease/3` と有限の処理時間・終了待機・ポーリングを表す。`max_processing <= 24 h` は実装上の上限で、`p6-outbox-plan.md:23,135` の **適格性検証の初期値 15 min** とは別。ランナーが適格性検証で 15 min を使うことを G06 で確認する。
- `crates/search-application/src/ports.rs:34–114` は Source フェンスと outbox トークンを別に保持する。`CurrentGenerationSnapshot` と `CompleteEventRequest` はキー、マニフェストダイジェスト、バンドルダイジェスト、ポインター改訂番号、Publish/Reuseモードを運び、Search完了処理の成功型と `Retry|Lost` を分ける。`crates/search-application/src/error.rs:11–14` は `FenceLost` と `CompletionUnknown` を区別する。対象アプリケーションAPI に SQLx 型・依存はない。
- `crates/search-application/src/indexing_service.rs:98–145` のルーティング対応表は Document イベント→Document、Folder イベント→Folder、`AccessPolicyChanged`→Document/Folder/AccessPolicy を許し、未知・誤集約は拒否する。現行生成側 (`document-application/src/events.rs:5–24`、`document-repository-postgres/src/targeted_events.rs:10–36`、`src/access_policy.rs:417–430,568–583`) と D4 の対象となるイベント群に一致する。`handle_delivery` はイベントID/フェンス ID と事前キャンセルを検査する。従来の `handle` による非 Document 系 `Ignored` と D4 メモリー経路は維持されている。

## 後続の明示的な受入条件

- `CurrentGenerationSnapshot` の 3 つの `Option` は型だけでは部分的なキー/ダイジェスト状態を禁止しない。これは計画どおりのポート形だが、S03 アダプターは DB CHECK と読取時の完全性検査を行い、`candidate.source_id == fence.source.source_id`、期待スナップショットのSource/キー・二つのダイジェスト・改訂番号、`ReuseCurrent` の現在値 READY を実行中のトランザクションで検証する必要がある（`p6-outbox-plan.md:171,189`）。型値だけをコミットの証拠にしない。
- `DocumentSourceEvent` は集約の型を運ばないため、S05 橋渡し処理が `DeliveryEnvelope` から変換する**前**に `validate_document_event_route` を必ず呼ぶ。S04/S05 は `IndexingOutcome::Ignored` を配送成功確定に写像せず、`Published|Unchanged|Duplicate` も S03 コミット済みの完了処理後だけ `Applied` にする（`p6-outbox-plan.md:198,207`）。Source 喪失やキャンセルは型だけでは証明できず、G05/S02/S04 の実際のリース更新・実行前確認と完了トランザクションが必須。
- `LegacyExhausted.first_ids` と `DeliveryPolicy` は公開値であり、モデル単体は診断 ID 件数や DB 一致を強制しない。G02 は `first_ids <= 32` と改訂番号・全値の DB ポリシー比較を実装・実DBテストで確認する（`p6-outbox-plan.md:99–102`）。

## 検証記録

- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test --locked -p outbox-delivery --lib policy::tests`: **3/3 PASS**（当時のこのレビュー中に実行）。
- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test --locked -p search-application --test port_contract`: **テスト実行前にビルド FAIL**。`crates/search-application/src/scoped.rs:810,841` の `RwLockReadGuard` が `.await` を越え、`BoxFuture + Send` を満たさない 2 件。別の実装担当の並行変更による範囲外の障害として扱い、S01 ポート・モデルの不合格とはしない。修正後に親側で同じ対象限定の4テストを再実行するまで S01 動的検証のGREEN は未確認。
- S01 範囲の観測 SHA-256: `ports.rs` `51abc463b9b8d1c53cd39ea158347ae33ce907cb8daadafdcb4fcd43328f2c57`; `indexing_service.rs` `a5bcf714ef09bc33c7c9c05f33e5f835405aa58f4e778aeca023123507baf18e`; `error.rs` `738741722a968168d1a2a4d2183c4e67150f73fc3d3117870316e81a5aaabfcb`; `tests/port_contract.rs` `82594997f5e68bd19611b5ffa161d7ad038cf42c9c7c94502a78dc853615c82b`。このハッシュと上記ビルド障害を分けて追跡する。

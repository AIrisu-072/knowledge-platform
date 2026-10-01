# P6-G01 / P6-S01 typed port・model 独立レビュー

## 判定と範囲

- **G01: 条件付き GO。** Generic model、allowlisted error、bounded config、typed `OutboxStore` は凍結計画の境界に一致する。ただし下記 P3 の retry vector を G05 runner 接続前に確定する。
- **S01: 静的 API 契約は GO、focused test は保留。** Search 側に SQLx 型を渡さず、二つの fence、expected current の key・二つの digest・revision、条件付き completion port を表現している。現在の `search-application` build 障害は S01 の編集範囲外であり、S01 の動的 GREEN receipt には使えない。
- 本判定は **port/model のみ**。DB claim/settle、Search completion transaction、P7 durable READY、generic ack、統合資格の成功は判定していない。P6-N01 は規範改訂の receipt であり、実装 receipt ではない。

照合した正本は `p6-outbox-design-revision-1.md` SHA-256 `c886a6b0933d6323b0fab7d41f0641e18332c6c74104025442a03609369d5366`、21 task の `p6-outbox-plan.md` SHA-256 `29638c077a29aebd007be4a4fd7c5fb7f7bfbf4a785ba0540782f98795811a80`（G01:90–93、S01:162–165）。対象 branch は `feat/search-platform-completion-core`、観測時 HEAD は `80a47960d025e4dfdea1eacade28b15d218725ff`、対象実装は uncommitted 作業木にある。

## 指摘

### P3 — retry jitter の exact vector が凍結計画と一致するか未確定

`p6-outbox-plan.md:90` の式は `SHA-256(event_id || attempt) mod (base/4+1)` だが、`crates/outbox-delivery/src/policy.rs:97–104` は digest の**先頭 8 byte だけ**を `u64` として剰余を取る。例えば unit test の event ID `21213141-7271-8281-8284-590452353602` と attempt 1、attempt を signed i32 big-endian と解釈すると、実装の jitter は 149 ms、256 bit digest 全体の剰余は 55 ms。`policy.rs:179–196` は範囲・単調性・安定性を確認するが、この違いを検知しない。

配送の安全性を破る差ではないが、凍結式を exact contract とするなら先頭 8 byte 解釈と attempt の byte encoding を計画に明記して承認するか、実装を式に合わせ、固定 vector test を追加する。G02 の DB writer は独立に進められる。G05 の backoff 接続・資格付け前に解決する。

## 確認できた契約

- `crates/outbox-delivery/src/model.rs:12–140` は payload を含む envelope、claim attempt/limit/token/owner/deadline、`Updated|Lost`、`Applied|KnownNoop|Retryable|Terminal`、7個の固定 error code、`PolicyMismatch|LegacyExhausted|InvalidConfig|StoreUnknown`、全 `OutboxStore` signature を計画どおり公開する。DB error/commit unknown と確認済み 0 行 `Lost` の区別も trait doc にある。generic crate は Search を import しない。`DeliveryEnvelope` の `Debug` は payload を含むが、この範囲に payload を記録する observer/logger は存在せず、現時点の漏えい finding にはしない。
- `crates/outbox-delivery/src/policy.rs:10–83` は policy revision と DB seed 相当の 8 attempts / 1–120 s lease / 1–300 s backoff、config の batch `1..=32`・in-flight `1..=8`・reap `1..=32`・`renew_interval < lease/3` と有限の processing/drain/poll を表す。`max_processing <= 24 h` は実装上の上限で、`p6-outbox-plan.md:23,135` の **qualification 初期値 15 min** とは別。runner が qualification で 15 min を使うことを G06 で確認する。
- `crates/search-application/src/ports.rs:34–114` は Source fence と outbox token を別に保持する。`CurrentGenerationSnapshot` と `CompleteEventRequest` は key、manifest digest、bundle digest、pointer revision、Publish/Reuse mode を運び、Search completion の成功型と `Retry|Lost` を分ける。`crates/search-application/src/error.rs:11–14` は `FenceLost` と `CompletionUnknown` を区別する。対象 application API に SQLx 型・依存はない。
- `crates/search-application/src/indexing_service.rs:98–145` の route matrix は Document event→Document、Folder event→Folder、`AccessPolicyChanged`→Document/Folder/AccessPolicy を許し、未知・誤 aggregate は拒否する。現行 producer (`document-application/src/events.rs:5–24`、`document-repository-postgres/src/targeted_events.rs:10–36`、`src/access_policy.rs:417–430,568–583`) と D4 の relevant event 群に一致する。`handle_delivery` は event ID/fence ID と事前 cancel を検査する。従来の `handle` による非 Document 系 `Ignored` と D4 memory 経路は維持されている。

## 後続の明示的な受入条件

- `CurrentGenerationSnapshot` の 3 つの `Option` は型だけでは部分的な key/digest 状態を禁止しない。これは計画どおりの port 形だが、S03 adapter は DB CHECK と読取時の完全性検査を行い、`candidate.source_id == fence.source.source_id`、expected snapshot の source/key・二つの digest・revision、`ReuseCurrent` の current READY を live transaction で検証する必要がある（`p6-outbox-plan.md:171,189`）。型値だけを commit 証拠にしない。
- `DocumentSourceEvent` は aggregate type を運ばないため、S05 bridge が `DeliveryEnvelope` から変換する**前**に `validate_document_event_route` を必ず呼ぶ。S04/S05 は `IndexingOutcome::Ignored` を成功 ack に写像せず、`Published|Unchanged|Duplicate` も S03 committed completion 後だけ `Applied` にする（`p6-outbox-plan.md:198,207`）。Source 喪失や cancel は型だけでは証明できず、G05/S02/S04 の live renew/preflight と completion transaction が必須。
- `LegacyExhausted.first_ids` と `DeliveryPolicy` は公開値であり、model 単体は診断 ID 件数や DB 一致を強制しない。G02 は `first_ids <= 32` と revision・全値の DB policy 比較を実装・実 DB test で確認する（`p6-outbox-plan.md:99–102`）。

## 検証記録

- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test --locked -p outbox-delivery --lib policy::tests`: **3/3 PASS**（このレビュー中に実行）。
- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test --locked -p search-application --test port_contract`: **テスト実行前に build FAIL**。`crates/search-application/src/scoped.rs:810,841` の `RwLockReadGuard` が `.await` を越え、`BoxFuture + Send` を満たさない 2 件。別 writer の並行変更による scope 外の障害として扱い、S01 port/model の不合格とはしない。修正後に親側で同じ focused 4 test を再実行するまで S01 dynamic GREEN は未確認。
- S01 scope の観測 SHA-256: `ports.rs` `51abc463b9b8d1c53cd39ea158347ae33ce907cb8daadafdcb4fcd43328f2c57`; `indexing_service.rs` `a5bcf714ef09bc33c7c9c05f33e5f835405aa58f4e778aeca023123507baf18e`; `error.rs` `738741722a968168d1a2a4d2183c4e67150f73fc3d3117870316e81a5aaabfcb`; `tests/port_contract.rs` `82594997f5e68bd19611b5ffa161d7ad038cf42c9c7c94502a78dc853615c82b`。この hash と上記 build 障害を分けて追跡する。

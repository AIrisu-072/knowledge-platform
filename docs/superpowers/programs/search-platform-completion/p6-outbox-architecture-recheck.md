<a id="p6-durable-outbox-delivery-worker--改訂-1-独立-architecture-再審査"></a>
# P6 永続 Outbox 配送ワーカー — 改訂 1 独立アーキテクチャ再審査

> 以下の状態・次の作業は当時の記録であり、現在の実行指示ではありません。現行の停止条件・実行可否は[最新の実行状態](../../execution/search-platform-completion-program-status.md)を参照してください。

[公開原文の固定リンク（commit `0ecf486719e3c9d71242e289a7564ad6d1032b3c`）](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-architecture-recheck.md)

本書は意味保存の日本語訳であり、原設計の再承認・実装/資格の追加ではない。既存ハッシュは当時の原文/証拠のものであり、訳文のハッシュではない。以下の状態・予定・合否は当時の記録として保持する。本文の行番号は同 commit の各原文を指し、従来の見出しアンカーも維持している。掲載コマンドは今回の翻訳作業では実行していない。

用語: Outbox は配送待ちイベントの永続表、Source は検索対象の情報源。リースは期限付き処理権、フェンスは古い所有者の更新を拒否する条件、エポックは所有権の世代番号、Search のイベント処理記録（receipt）は索引処理結果の記録、検証の receipt は試験・実行結果の証拠記録を指す。CAS は期待する現値との一致を条件にした更新、GC は不要世代の回収、DLQ は打切りイベントの保管先。Search / Domain / Document / Folder / AccessPolicy / Projection / Unit / Graph などの構成名・型名は識別のため維持する。

- 判定: **GO（P6 設計のアーキテクチャゲート）（P1/P2 残件なし）**。設計凍結、実装、実 PostgreSQL による資格確認、P7 接続の完了判定ではない。
- 対象: [審査対象の固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-design-revision-1.md)の SHA-256 `c886a6b0933d6323b0fab7d41f0641e18332c6c74104025442a03609369d5366`（[読者向け日本語訳](p6-outbox-design-revision-1.md)は別の文書バイト列）、既報 `p6-outbox-architecture-review.md`、現行 Domain Outbox / D4 Search コンシューマー、P3 改訂の共有ロック境界。審査中に設計文書のコミットが進んだため、上記 SHA を審査対象の同一性とする。
- 方法: 設計・既存コードの静的照合、および PostgreSQL の [行ロック](https://www.postgresql.org/docs/current/explicit-locking.html) / [SKIP LOCKED](https://www.postgresql.org/docs/current/sql-select.html) 契約の確認。新 SQL の実行、障害注入、性能測定は行っていない。

## 既報 P2-1〜4 の再判定

| 指摘 | 判定と根拠 |
| --- | --- |
| P2-1 処理権取得後の配送実行待ちでリース失効 | **閉鎖**。改訂 31–56 行は空き実行枠確保後に `limit <= free_permits` で処理権を取得し、Search は 1 件に固定する。配送実行直前の `renew` が現在のトークン・未完了・DB 時刻での期限を検査し、0 行または応答不明ならハンドラーを呼ばない。リース更新中の喪失は取り消し / 結果不明とする（58–60 行）。`SKIP LOCKED` をハンドラー期間の排他と誤認していない。 |
| P2-2 最終処理権取得後の異常終了で行が永久滞留 | **閉鎖**。行ごとの `attempt_limit` と DB 方針を固定し、方針不一致や旧上限行を安全側に拒否する（18–20 行）。起動時・毎ポーリングサイクルの処理権取得前・終了時の完了待機終端に上限付き `reap_exhausted` を実行し、期限切れ最終試行を `delivery_unknown_at_limit` として原行を残したまま一度打切り状態へ移行する（102–126、174 行）。結果不明を失敗確定と偽らない。 |
| P2-3 プロセス内限定ゲートでは Source 単位を排他できない | **閉鎖**。改訂 14、22、130–145 行は共有 DB の Source リースを Outbox の処理権取得前に取得し、全プロセスで同一 Source の整合化を同時に 1 件に制限する。取得ごとの単調な `fence_epoch`、所有者トークンと期限によるリース更新/解放、待機側が処理権を取得しないことを規定する。現行 `DocumentOutboxIndexer::gate` がインスタンス内限定（`crates/search-source-document/src/outbox.rs:642-648,701-712`）である点を正しく区別している。 |
| P2-4 古いハンドラーが新しい処理記録・現在の索引世代を上書き | **閉鎖**。改訂 22–24、147–170 行は Domain Outbox、Source ポインター、処理記録を同じ PostgreSQL DB に置き、公開と処理記録を同一の短いトランザクションで両リース / エポック、現在のポインター、READY を条件付き検査する。処理記録は低いエポックからのみ更新し、同エポックはキー / ダイジェスト完全一致時だけ冪等とする。`Published` だけでなく `Unchanged` / `Duplicate` も結合ポートを通る。旧/GC 済み処理記録は保持用の固定参照と見なさず、現在のポインターと READY の一致を再検証する。現行の無条件 `IndexingReceiptStore::put`（`crates/search-source-document/src/outbox.rs:55-58,821-845,910-921`）を永続保存を行う実装に流用しない。 |

## 境界の照合

- 現行 `outbox_events` はリース / DLQ 列を持たず、`audit_outbox_events` は別表である（`crates/document-repository-postgres/migrations/0001_document_authoritative_core.sql:74-101`）。改訂 18–20 行の NULL を許容する列・CHECK・旧行保持と、10 行の Domain / Audit 責務分離は既存を保持した追加型のマイグレーションの契約として整合する。イベント / ペイロード / `delivered_at` の既存値を移行で再作成・更新しない。
- Search 接続処理は `Document` / `Folder` / `AccessPolicy` の型付きイベントを許可リストに照合し、ペイロードではなく現行 Source のスナップショットを読む（改訂 12 行、`crates/search-application/src/indexing_service.rs:43-85`）。汎用 P6 は配送状態だけを所有し、Search 処理記録は Search が所有する（改訂 10、192–194 行）。
- 複合トランザクションのロック順は Outbox → Source（改訂 149 行）に、P3 の Source → 索引世代キー順 → リース（`p3-graph-design-revision-1.md:52-62`）を接続できる。手動再構築は Outbox 行を持たず Source から始める。ハンドラーが索引を構築している間に Domain 行ロックを保持しない。リース喪失時の取り消しは協調的であり、既に走る外部構築の即時停止を保証しないため、可視状態の変更を両フェンス付き CAS に限定する記述（改訂 130、168、174 行）が必要十分な設計境界である。
- P3 の別再審査は増分構築の基点 / 対象の永続的な構築保護を未解決としている（`p3-graph-architecture-recheck.md:18-28`）。P6 の候補準備領域保護（改訂 150 行）と P7 の GC / ポインター / 評価用保持を統合する際、その保護と P3 の `pointer_revision` 条件を保持する必要がある。これは P3/P7 構成の未完了事項であり、既報 P6 四件の再発や P6 単独の新規 P2 とは判定しない。

<a id="次の-exact-action"></a>
## 次に行う具体的な作業

上記 SHA の改訂を P6 の設計凍結候補として扱い、同一 DB の両フェンス、処理記録 / GC の意味、Source リースの処理権取得前取得、DB 方針と旧上限行、規範上の `aggregate_version` / `processing_state` 差分を凍結 / 計画に反映する。P6 の本番完了は、計画された実 PostgreSQL の異常終了・再配送・フェンス・マイグレーション資格確認と P7 の永続基盤の構成を確認した後に別判定する。本再審査から実装試験済み、P3 設計ゲート通過、マージ / デプロイ済みとは推定しない。

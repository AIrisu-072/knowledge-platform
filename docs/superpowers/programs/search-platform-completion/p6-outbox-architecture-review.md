<a id="p6-durable-outbox-delivery-worker--独立-architecture-review"></a>
# P6 永続 Outbox 配送ワーカー — 独立アーキテクチャレビュー

> 以下の状態・次の作業は当時の記録であり、現在の実行指示ではありません。現行の停止条件・実行可否は[最新の実行状態](../../execution/search-platform-completion-program-status.md)を参照してください。

[公開原文の固定リンク（commit `0ecf486719e3c9d71242e289a7564ad6d1032b3c`）](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-architecture-review.md)

本書は意味保存の日本語訳であり、原設計の再承認・実装/資格の追加ではない。既存ハッシュは当時の原文/証拠のものであり、訳文のハッシュではない。以下の状態・予定・合否は当時の記録として保持する。本文の行番号は同 commit の各原文を指し、従来の見出しアンカーも維持している。掲載コマンドは今回の翻訳作業では実行していない。

用語: Outbox は配送待ちイベントの永続表、Source は検索対象の情報源。リースは期限付き処理権、フェンスは古い所有者の更新を拒否する条件、エポックは所有権の世代番号、Search のイベント処理記録（receipt）は索引処理結果の記録、検証の receipt は試験・実行結果の証拠記録を指す。CAS は期待する現値との一致を条件にした更新、GC は不要世代の回収、DLQ は打切りイベントの保管先。Search / Domain / Document / Folder / AccessPolicy / Projection / Unit / Graph などの構成名・型名は識別のため維持する。

- 判定: **NO-GO（P1 なし、P2 4 件）**。設計案の凍結前に以下の配送・処理記録契約を確定する。
- 対象: `p6-outbox-design.md`（2026-09-30 の DRAFT）、現行 Domain Outbox スキーマ/生成側、Search D4 コンシューマー、関連規範仕様。実装・配備の合格判定ではない。
- 方法: リポジトリの読み取り専用照合と PostgreSQL 公式文書の SQL の意味の確認。新 SQL・ワーカーは存在しないため、実 PostgreSQL による資格確認は未実施。

<a id="blocking-findings"></a>
## 進行を阻む指摘

<a id="p2-1-claim-済みで-dispatch-待ちの-lease-が失効し得る"></a>
### P2-1: 処理権取得済みで配送実行待ちのリースが失効し得る

設計案 `p6-outbox-design.md:68,70,77,99` は `batch_size <= 32`、`max_in_flight <= 8` を許し、リース更新を「長いハンドラーの間」と記す。32 件の処理権を同時に取得して 8 件だけをハンドラーに渡すと、残り 24 件はセマフォ待ちの間に 120 秒のリースが失効し得る。他プロセスはそれらの処理権を再取得でき、元プロセスが後から同じイベントを配送実行すると不要な重複と試行数消費が起こる。`FOR UPDATE SKIP LOCKED` が保護するのは処理権取得トランザクション中の行だけである（[PostgreSQL SELECT](https://www.postgresql.org/docs/current/sql-select.html)）。

**修正条件:** 配送実行枠を確保してから `claim(limit <= free_permits)` するか、処理権取得コミット直後から待機中を含む全リースを更新し、配送実行前にもトークン/期限を再検証する。前者を初期契約とすれば単純。`batch=32 / in_flight=8 / handler > lease` の実 DB 試験で、処理権取得済みで未開始のイベントの重複処理と上限到達が起きないことを確認する。

<a id="p2-2-最終-claim-後の-crash-を-dlq-へ移す実行契約がない"></a>
### P2-2: 最終処理権取得後の異常終了を DLQ へ移す実行契約がない

設計案 `p6-outbox-design.md:68,73` では `attempt_count >= max_attempts` を処理権取得から除外し、別の `reap_exhausted` で打切り状態へ移行する。一方、実行ループ `:99` は `poll -> claim -> dispatch -> renew -> settle` だけで、回収処理の起動時・周期・停止時の実行を定めていない。最終試行の処理権を取得した直後にプロセスが落ちると、リース失効後も `delivered_at/dead_lettered_at` が NULL のまま処理権取得対象から永久に外れる。

**修正条件:** 起動時と周期実行に上限付き `reap_exhausted` を明記し、DB 時刻で `lease_expires_at <= clock_timestamp()`、未完了、上限到達を確認するロック付き/フェンス付き更新と警報を契約化する。複数ワーカーの最大試行設定差で早期 DLQ が生じない設定境界も固定する。最終処理権取得コミット直後の強制終了と再起動を実 PostgreSQL で試験し、原行が残ったまま一度だけ `DEAD_LETTER` になることを確認する。

<a id="p2-3-source-あたり-in-flight-1は複数-process-で成立しない"></a>
### P2-3: 「Source あたり同時処理 1」は複数プロセスで成立しない

設計案 `p6-outbox-design.md:54,68,77,99` のセマフォはプロセス内限定と読める。2 ワーカーが同じ Document Source に属する別イベントの処理権を取得すれば、それぞれが全 Source 再構築を並列実行できる。D4 の `DocumentOutboxIndexer::gate` もインスタンス内限定（`crates/search-source-document/src/outbox.rs:642-648,701-704`）。索引世代の CAS と最大 3 回の再読（同 `:705-712`）は射影の破壊を防ぐが、障害のない状態での集中発生で CAS 敗北と再試行を繰り返し、`max_attempts` に達してイベントを DLQ に送る可能性は残る。`aggregate_id` は Document/Folder 等であり、Source 単位の配送実行キーではない（`crates/document-repository-postgres/src/targeted_events.rs:10-15`）。

**修正条件:** P6 初期接続で Search 接続処理を単一の稼働中プロセスに限定する配置契約、またはプロセス間の Source 単位の排他・処理の集約契約を明示する。汎用ワーカーの 2/4/8 台処理権取得試験とは別に、同一 Source のイベント集中発生とワーカー交代を実 DB + Search ハンドラーで検証し、CAS 競合が正常イベントを DLQ にしないことを示す。P7 の共有実行基盤は別途必要であり、プロセス内限定実行基盤の複製を本番成功と数えない。

<a id="p2-4-lease-失効後の古い-search-handler-が-durable-receipt-を上書きできる"></a>
### P2-4: リース失効後の古い Search ハンドラーが永続処理記録を上書きできる

設計案 `p6-outbox-design.md:70,81-83,95` は旧トークンの Outbox 配送確認/失敗確定をフェンスするが、ハンドラーが行う Search 公開/処理記録書込みはフェンスしない。現行 `IndexingReceiptStore::put(event_id, receipt)` には条件付き更新契約がなく（`crates/search-source-document/src/outbox.rs:47-58`）、`Published` と `Unchanged` の後に呼ばれる（同 `:821-845,897-921`）。例: 旧ハンドラーが G1 を公開して処理記録の put（保存） 前に停止し、リースを再取得した新ハンドラーが更新後の G2 を公開・処理記録の put（保存）・配送確認した後、旧ハンドラーが G1 の処理記録を put（保存）できる。イベント処理記録は古い索引世代を指し、設計案 `:83` の再起動後の整合条件と衝突する。G1 が GC されればその参照は読めない。

**修正条件:** P7 調整器と Search 処理記録の関係を凍結で定義する。少なくとも処理記録の `put` が新しい現行索引世代を古い索引世代で上書きできない条件と、GC 済み索引世代を指す歴史的処理記録の扱いを決める。リースが失効した間の二重ハンドラー、公開後の停止、G2 配送確認後の旧 put（保存）、再起動/GC を実 DB で再現し、現在の射影と処理記録の契約が保たれることを確認する。単なる `event_id` PK / 無条件挿入・更新では足りない。

<a id="確認済みの境界と-qualification-条件"></a>
## 確認済みの境界と資格確認の条件

- 現行 `outbox_events` は `event_id` PK、`available_at`、`attempt_count`、`delivered_at` を持つがリース/DLQ 列はない（`crates/document-repository-postgres/migrations/0001_document_authoritative_core.sql:74-84`）。`audit_outbox_events` は別表（同 `:86-101`）。既存を保持した追加型のマイグレーションと既存行保持の方針は適切。`0009` は現行の最大 `0008` の次である。
- 現行生成側は業務トランザクション内に Domain イベントを記録する（例: `crates/document-repository-postgres/src/repository.rs:177-193`、`targeted_events.rs:18-41`）。Search 接続処理がペイロードを正本とせず、D4 が現行スナップショットを再読する境界は妥当。`AccessPolicyChanged` は Document と Folder の対象にも発生するため、許可リストは一対一対応と仮定しない（`access_policy.rs:417-431,568-580`）。
- `SKIP LOCKED` はキュー型の表の複数コンシューマーに使えるが、厳密な集約順序を与えない。設計案の `available_at, occurred_at, event_id` を優先順とする説明は妥当（[PostgreSQL SELECT](https://www.postgresql.org/docs/current/sql-select.html)）。期限の境界は `claim <= expiry`、`renew/ack/fail > expiry` として実 DB でちょうど同時刻の 0 行更新、旧トークン、配送確認コミット応答不明を試験する。`clock_timestamp()` は SQL 文内でも変化する（[PostgreSQL 日時](https://www.postgresql.org/docs/current/functions-datetime.html)）。
- `aggregate_version` と物理 `processing_state` は規範の一覧（`spec/data/transaction-consistency-requirements-v0.md:760-772`）にあるが、現行生成側/表に共通値はない。設計案 `:17,46,50,114` の読み取りモデル・非保証の規範差分は凍結で明示的に決裁し、既存ペイロードから一律埋戻ししない。
- `traceparent/tracestate` は旧生成側では空のままとし、格納前と使用前に妥当性・長さ・機密値の許可リストを確認する。`tracestate` の構文適合だけを機密性の保証にしない。ペイロード/操作主体をログ・メトリクスラベルに出さない方針と、Audit Outbox の配送確認非更新を縦断試験で確認する。
- 実 PostgreSQL 用の新ワーカー/マイグレーションがまだないため、このレビューは SQL 実行結果や処理量を証明しない。P6 の完了には `p6-outbox-design.md:109-116` のマイグレーション保存性、複数ワーカー処理権取得/フェンス、異常終了/再試行/DLQ、Search 処理記録/配送確認、終了処理の対象を限定した資格確認の記録が必要。

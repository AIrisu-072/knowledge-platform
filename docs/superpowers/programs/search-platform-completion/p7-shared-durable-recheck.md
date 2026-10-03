# P7 共有 durable generation 基盤 — 改訂 1 独立再審査

Status: **GO — 元レビューの 5 件は設計上解消。共有基盤の設計 Freeze 可**（2026-09-30）。判定対象は [元設計](p7-shared-durable-design.md) に [改訂 1](p7-shared-durable-revision-1.md) を優先適用した契約である。production migration、SQL role/trigger、実 PostgreSQL RED→GREEN、P3 backend 選定、Graph READY/publish、P6 縦断、最終 P7 runtime/HTTP/運用の GO ではない。

## 固定入力と方法

- 元設計 SHA-256 `99a989e30ec77ea5f79915a3db22ecd31b910decbabc825c2ae8bca6f1041a3b`、元 [独立レビュー](p7-shared-durable-review.md) の指摘 1–5、改訂 1 SHA-256 `2e2f1a24972f2020278cfe7fe37c1726ae280e936685ef74299c1c5c2a8641fe` を再読し、元設計は未変更と照合した。
- P5 [source-neutral 改訂 2](p5-api-contract-revision-2.md) SHA-256 `81c8be90375bd2461fa8179ef7d5fdb0705c1b02431dee918d99ac26372901d9`、P1 Extraction/KnowledgeUnit Freeze、P3 Graph Freeze・build guard・FK-safe cleanup と計画、P6 Outbox Freeze と計画、現行 `search-application` port/scope/Remote ledger を静的照合した。P3 計画 SHA-256 `cc6a6ddee5004c1da419f3d95965a98f71a4b8ea2e081a5d9a36267203e9c86f`、P6 計画 SHA-256 `29638c077a29aebd007be4a4fd7c5fb7f7bfbf4a785ba0540782f98795811a80` は元レビューの固定入力と一致する。SQLx 0.9.0 の `Migrator::dangerous_set_table_name` は pinned local source `sqlx-core-0.9.0/src/migrate/migrator.rs:111` で存在を再確認した。
- 方法は文書・現行 port の read-only 照合である。DB migration、role 権限、trigger の実行、Rust build/test、CI は本再審査で実施していない。

## 指摘ごとの判定

| 元レビュー | 設計上の閉鎖根拠 | 判定 |
| --- | --- | --- |
| 1. 別 event/旧 epoch/MANUAL READY 候補の event completion 混入 | 改訂 §1:15–37 は `stage_origin` と event ID/Source epoch の complete-or-null CHECK・不変性、EVENT/MANUAL の private handle 分離、locked outbox/Source 行からの再取得、candidate の保存 snapshot/activation/同一 target guard と READY を pointer/receipt と同じ transaction で照合する。`ReuseCurrent` は候補 stage を受けず、現在の READY/権限/retention を再検査する。 | **CLOSED** |
| 2. Full guard の target 束縛と失効 fence | 改訂 §2:41–64 は target に不変 token/fence を保存し、guard からの複合 FK と一意制約を置く。P7/Graph child DML、READY、renew、publish は保存 binding と DB clock の未失効 guard を再検査し、失効 target の再発行を禁じる。abort/expiry は `DELETING → guard DELETE → child DELETE → parent DELETE` を一 transaction に保ち、恒久 identity が key 再利用を拒む。P3 incremental の base/target guard と同じ Source fence counter・lock 順を維持する。 | **CLOSED** |
| 3. P3 `stage_full` が別接続で Graph parent を登録できる | 改訂 §3:68–72 は P7 coordinator の一接続・一 transaction を production の唯一の登録入口とし、Graph parent/guard 登録失敗を全 rollback する。production `stage_full_registered` は登録済み handle で子 batch のみを stage し、旧 parent-creating port は isolated PoC/fixture 専用、production role は親 INSERT 不可とする。P3-G01/G04/C01 の計画変更を着手前条件にした。 | **CLOSED** |
| 4. durable pin の発行先 scope が保存されない | 改訂 §4:76–80 は tenant owner、非秘密 `actor_scope_ref`、registration/visibility/access revision を lease に不変保存する。renew/return/release は保存 lease と現在の trusted actor/evaluation scope、P4/P5 current gate、Source activation/権限を再照合する。再起動後に reference を解決できなければ fail closed、Remote RAM lease は分離する。 | **CLOSED** |
| 5. Source kind と Remote desired 集合の権威が不明 | 改訂 §5:84–90 は `source_kind` allowlist と owner/kind の tombstone 後も不変な global ledger、Document/Remote 共通 serial lock を定義する。P5 改訂 2 §2 の namespace 別 `CompleteDesiredRegistrations` を production 契約に採り、trusted composition root の全 tenant・同 revision の完全 snapshot と DTO exact equality、canonical set digest を比較する。revision/digest は namespace 別に保持し、tombstone は同 namespace の行だけに限定する。 | **CLOSED** |

元レビューの named real-PG regressions 16 件は改訂 §6:96–104 に全件引き継がれている。これは **試験条件の確定**であって試験結果ではない。特に実 role の直接 DML 拒否、期限切れ境界、別接続での競合、guard DELETE 後の fault rollback、旧 ownership 行の backfill failure を実装時に確認する。

## Freeze と実装の境界

- 設計 Freeze は元設計と改訂 1 の優先順を exact SHA で固定する。現行 `CompleteEventRequest` の単一 `candidate` field、P3 計画の parent-creating `stage_full`、P4 Remote-only `SourceRegistrationLedgerPort` は改訂後の production signature ではない。P6-S01/S03、P3-G01/G04/C01、P4-02、P7 migration/port の計画・型・role を改訂 1 §§1–6 と P5 改訂 2 §2 に揃えてから各実装に着手する。P5 改訂 2 自体の独立判定・Freeze も別に必要である。
- P3-P04 は現時点で production backend 昇格 NO-GO。`GraphReceiptMappingV1` の P1 staged-input と P3 content の二つの canonical encoder/golden vector も未成立であり、P3 isolated PoC の PASS や digest 値の流用を資格にしない。Graph 非依存の共有 PG schema/port だけ局所的に進め、Graph production migration・READY・publish は停止する。
- 元レビュー §再判定条件と改訂 §6 の real-PG/実 role/独立接続 RED→GREEN、P1 composite・lexical seal、unknown commit、current/pin/guard/GC、別 DB restore は **実装受入ゲート**に残る。ここで未実施であることを、解消済みの設計指摘を再度 NO-GO にする理由にはしない。最終 P7 acceptance と exact-head hosted qualification は別判定とする。

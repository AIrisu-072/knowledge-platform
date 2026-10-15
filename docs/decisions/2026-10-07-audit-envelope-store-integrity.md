# Audit Infrastructure v1：envelope・Store・integrity・staging接続の判断

## 状態

ADOPTED FOR v1 IMPLEMENTATION（Audit Infrastructure v1 track、2026-10-07）。設計の独立review 3回（最終reviewの指摘は改訂3で反映）を経て、依頼者の実装指示の範囲で採用する。Document担当・Search担当には、設計§13のhandoffで影響の確認を求める。依頼者の実装指示（Audit Outboxから監査Storeまでの配送・保存・検証の完成）と TC:797（`audit_outbox_events` の行・配送状態・ackは独立したAudit経路が所有する）に基づく。設計の詳細は[配送・保存・検証設計](../superpowers/specs/2026-10-07-audit-infrastructure-v1-delivery-design.md)。Document担当・Search担当への影響は同設計§13のhandoffで共有する。本番採用・本番DB移行・deployは意味しない。

## D1. CloudEvents envelopeは自前の最小実装とする

- **判断：** `audit-core` に、CloudEvents 1.0.2 structured JSON formatの閉じたenvelopeを実装する。属性は `specversion`、`id`、`source`、`type`、`subject`、`time`、`datacontenttype`、`dataschema`、`data`。未知の属性とextension attributeは拒否する。
- **根拠（AC:707）：**
  - `cloudevents-sdk` 0.9.xはLTS:265でPOC REQUIREDであり、productionへ入れられない。
  - P7 PoCは `experiments/` に存在しない。
  - envelopeは属性集合が閉じた単純なJSONで、SDKの抽象化（transport binding等）を要さない。
- **代替案：**
  - 先にP7 PoCを実施する：Audit配送を止める理由にならないため不採用。
  - SDKをexperimentsで使う：production経路で使えないため不採用。
- **条件：**
  - LTS §6.3のPoC受入項目（v1.0 JSON round-trip、Audit Event JSON Schemaとの整合、id/source/type/subject/timeの安定、event IDによるidempotency、SDK型をDomainへ漏らさないこと）を、自前実装のconformance試験として採用する。
  - `cloudevents-sdk` はPOC REQUIREDのまま残す。将来SDKを採用する場合は、adapterで置き換える。

## D2. Audit StoreはPostgreSQL 18＋SQLx（暫定採用）

- **判断：** v1/PoCのAudit Storeを、既存の選定済みstack（PostgreSQL 18、SQLx 0.9）で実装する。別database・別schema（`audit_store`）・別migration ledger（`audit_store_sqlx_migrations`）とし、本番前に再選定するゲートを残す。
- **LTS:268のDEFERRED条件に対する根拠：**
  - retention：方針をデータとして持つ（版付き・不変）。年数は固定しない。既定は失効なし（OA §26）。
  - tamper-evidence：D3の方式Bを採る。
- **新しい外部運用基盤・有償サービス・新規依存は追加しない。** backup/restoreは既存の `pg_dump` / `pg_restore` で行う。

## D3. integrityはwrite-time hash chain＋event digest＋外部checkpoint

- **判断：**
  - 各eventのdigestは `sha256(envelope jsonb text)` とする（`kp-audit-jsonb-sha256-v1`）。
  - chainは `sha256('kp-audit-chain-v1' || prev_chain || seq || event_id || digest)`。genesisは `sha256('kp-audit-chain-genesis-v1')`。
  - checkpoint（epoch, seq, chain）は帯域外に保管する。
  - 真正性の主張は、DB外で連続したchainを再計算し、帯域外checkpointと一致した場合に限る。
- **比較：**

  | 方式 | 評価 |
  |---|---|
  | A. digestのみ | 削除・同時改変を検出できない |
  | B. 本方式 | 採用。commit順を確定するhead lockで既に直列化されているため、costはinsertごとのsha256 1回で済み、checkpointはO(1)になる |
  | C. 検証時Merkle | checkpoint作成がO(n)で、行単位の自己検証性が無い |
  | D. 署名・WORM・外部anchor | 鍵管理と新しい運用基盤が要る |

- **限界：**
  - DB owner（superuser）はchainを全体再計算できる。これは帯域外checkpointとの照合でのみ検出できる。
  - DB ownerは、最後のcheckpoint以降の任意のsuffix（Store生成の閲覧記録を含む）を失わせ、それを「復旧」として装うこともできる。v1はこれを可視化し、帯域外の記録（restore時のepoch遷移の追記）との照合に依存させる。抵抗するにはDが要る。
  - 配送前のstagingは、Document DBの特権者が改変し得る（設計§5.3）。
  - Dは将来の拡張とする。

## D4. Document所有の `audit_outbox_events` への接続

- **判断：** `audit_relay` ledgerのmigrationが、`public.audit_outbox_events` に次を追加する。
  1. `AFTER INSERT` 登録trigger（同一transactionで配送状態を作る。失敗時は業務もrollbackする）
  2. `UPDATE` / `DELETE` / `TRUNCATE` を拒否するappend-only guard
  3. `audit_relay.deliveries` からのFK
  4. `BEGIN ATOMIC` 本体のsource digest関数（依存列を追跡する）
- **根拠：** TC:797がstaging行・配送状態・ackをAudit経路に割り当てている。列の追加や既存migrationの変更は行わない。
- **意図的な制約：**
  - 今後のDocument migrationで、digest対象列をDROP・型変更するとmigrate時に失敗する。
  - この表に触れるDocument migrationには、Auditのreviewが要る。
  - 既存のowner DELETEを前提とした試験（`document_history_projection.rs:88`）は、Document単体のDBでのみ有効になる。
- **migrate順：** Document ledger → `audit-relay migrate`（列・型の事前検査付き）。`audit_outbox_events` に対する `DROP COLUMN ... CASCADE` は禁止する。

## D5. 自由記述reasonはStoreへ複製しない

- **判断：** reasonを持つ7種（withdrawn、publication.ended、metadata.changed、moved、folder.created/renamed/moved）は、Storeへ配送する。理由文そのものは複製せず、`{provided, utf8_bytes, text_retained: "source_systems"}` だけを記録する。Storeへ渡すsource commitmentはsalt付きである。Storeの読者は理由文を取得できず、推測確認もできない。ただし、正確なUTF-8 byte数による長さの区別だけは残る。
- **根拠：** OA:646「許可された理由分類」「無制限の入力を複製しない」、OA §19、依頼者の「顧客データ等を無条件保存しない」。
- **旧判断（PR44/45）との違い：**
  - PR44/45は、reasonを持つeventを配送しない（quarantineする）と判断していた。この判断では取下げ・metadata/ACL変更がStoreへ届かず、今回の要求を満たさないため、置き換える。
  - legacy rowに無いreasonを復元したかのように表現しない点は、引き継ぐ。

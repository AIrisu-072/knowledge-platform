# Audit Infrastructure v1：実行状況

## 2026-10-07 — 単位A（event契約）の実装・review・exact-head CI

- 設計は改訂3（`8254d76`）で確定した。独立reviewは3回行った。
  - review 1：Critical 3件
  - 再review：Critical 1件
  - 最終review（設計2観点＋code 3観点）：Critical 1件（portが改訂1の三分類のまま）

  全件を反証付き検証の後に反映した。主な変更：
  - 閲覧intentのcommit判定は `pg_xact_status`＋flush待ちとする。
  - 主体はsession_userへ束縛し、role・credential行列を定める。
  - restore時の権限posture、recovery mode、帯域外のepoch記録。
  - 二分類の失敗model：Storeの構造化verdict以外は、すべて外部障害として試行を返却する。
  - rebindは廃止し、`planned_move` epochで扱う。
  - reconciler role。
  - ingestはsource service主体に限定する。
  - 失効証拠のDB外検証。
- 単位Aのcommit：
  - `7578dfc`：audit-core、catalog、生成schema
  - `cb22f89`：control 14種、DB外の復旧判定
  - `749c93d`：最終reviewの修正（二分類のport、golden投影pin、adapter定義、nil client ID、principal文字種、jsonb相当長、export行のexpired_by_seqと失効・epoch・originの検証）
  - `f369261`：最新main（Organization U1〜U4、local workspace runtime、Folderアクセス設定）を統合。active.mdのconflictは双方の節を保持して解消
- ローカル検証（`f369261`）：
  - `cargo test -p audit-core`：118件PASS（lib 28、catalog 13、chain/export 38、envelope 12、golden 2、legacy 15、schema 7、store_port 3）
  - clippy `-D warnings`、fmt、architecture-lint：PASS
  - `cargo metadata --locked`：PASS
- hosted CI：PR98のexact-head `f369261` で、CI run 37619375294がrequired-checkを含む全項目SUCCESS（Organization D2系はskip）。
- 計画からの逸脱（承認状態）：
  - control typeは12→14種：`access.closed`、`retention.expire_refused`。checkpointは `integrity.verified` の trigger=checkpoint で表す。fingerprint_reboundは改訂3で廃止。設計§4.5を正本の要約として更新済み。計画単位A手順2も14種へ修正済み。
    - 承認状態：依頼者の実装指示の範囲内で本trackが採用、設計改訂3・独立reviewで確認済み（依頼者による個別承認ではない）。
  - control eventのlist kind（`event_type_list`、`source_list`）の上限は16（32 KiB上限と設計§10.3のfilter event_types≤16に合わせる）。retention selectorも16件までになる（設計§9とREADME §kindに記載）。当初の自由文字列 `identifier_list` は、独立検証の指摘S2により閉じたkindへ置換した。
    - 承認状態：依頼者の実装指示の範囲内で本trackが採用、設計改訂3・独立reviewで確認済み（依頼者による個別承認ではない）。
  - golden pin：計画単位A手順7に追記（entryを `<fixture>@<入力行hash>` に変更、旧sectionはdigestで凍結）。現在のsection 1はkey形式だけを移行し、全31件のdigestは不変。
    - 承認状態：依頼者の実装指示の範囲内で本trackが採用、設計改訂3・独立reviewで確認済み（依頼者による個別承認ではない）。
  - DB外判定の厳格化（独立検証の指摘S1・S3への修正で追加。設計§8:457より厳しい）：`unverified_expiry_evidence > 0` または認証範囲外の失効証拠があるreportは `Authentic` にしない（`UnverifiedExpiry`）。headより前のcheckpointだけでは `AuthenticThrough { seq }`。
    - 承認状態：依頼者の修正指示の範囲内で本trackが採用。設計本文（§8）へ反映済み。修正後の確認は単位Aの最終確認reviewで行う。
  - 束縛主体の無いcontrol event（unboundの拒否、bootstrap）のactorを `{issuer: "db_role", principal_id: session_user}` と定めた（設計§10.2とREADME §control eventに記載）。
    - 承認状態：依頼者の修正指示の範囲内で本trackが採用。設計本文へ反映済み。修正後の確認は単位Aの最終確認reviewで行う。
- 単位B（Store・relay）：
  - Store crate（39件）とrelay crate（36件）は別worktreeで実装済み。
  - 改訂3と新しいcore APIへの追従は、別worktreeで実施中（未push）。
- 次のexact action：
  1. 単位Aのfix確認reviewを完了する。
  2. PR98の本文を更新し、mainへmergeする。
  3. main CIを確認する。
  4. branchを最新mainから作り直し、単位BのDraft PRを作る。

## 2026-10-07 — 最新main基点で再開、設計改訂1を再review中

- 基点はmain `d515aa38085c9ed7e41f8103d9c1a6c576025fd4`（push CI 37562024089 SUCCESS）、branchは `claude/cool-darwin-7xh893`、Draft [PR98](https://github.com/AIrisu-072/knowledge-platform/pull/98)。旧Draft PR44（設計）・PR45（schema）は基点が30 merge古く、方針（reasonを持つeventを配送しない）が今回の要求と衝突するため、stackとしては使わない。PR45のcrateは現mainで24/24 PASSした（調査時の一時worktreeで確認）。重複key parser・catalog形式の考え方だけを流用する。PR44/45は、置換PRが統合可能になった時点で理由を付けてcloseする。
- 調査範囲：7並列reader＋critic。producer 21種（INSERT 13箇所）、staging DDL・権限、汎用 `outbox-delivery`、規範要求、PR45、runtime/CI、Organization/Searchの境界。主な事実：
  - 配送・Store・integrity・retention・閲覧監査はいずれも欠落している。
  - 自由記述 `reason` を持つeventが7種あり、withdraw/endの理由文には上限が無い。
  - 通常のACL変更はreasonを持たない。
  - `authorization.denied` の試験は0件。
  - W3C trace_idは保存されていない。
  - Searchは別のaudit outboxを持ち、未配送である。
  - Workの `event_staging` にはconsumerが無い。
- 設計：[配送・保存・検証設計](../specs/2026-10-07-audit-infrastructure-v1-delivery-design.md) 改訂1、[決定記録](../../decisions/2026-10-07-audit-envelope-store-integrity.md)、[実装計画](../plans/2026-10-07-audit-infrastructure-v1-delivery.md)。
- 独立review 1（security / correctness / spec、指摘ごとに反証を試みる検証付き）の結果：
  - Critical 3件：閲覧記録がrollbackで消える、`retain_days=NULL` で本文が削除される、主体を引数で詐称できる。
  - Important 20件超：search_path/PUBLIC EXECUTE、role未定義、ingestの偽装、salt無しdigest、genesis、再投影の偽conflict、障害時の試行消費、replayの試行予算、restore gate、retentionの偽装、correlation写像ほか。
  - 反証された指摘は0件。全件を改訂1へ反映した。再reviewは実行中。
- 検証：設計文書のみ。PR98の旧head `5ad05d5` はhosted CI全項目SUCCESS（設計候補のみで、実装の資格ではない）。
- 環境：Docker daemonを起動した。Docker Hubが429（rate limit）を返したため、同一imageを公式mirror `mirror.gcr.io/library/postgres:18.6-bookworm` から取得し、`postgres:18.6-bookworm` としてtagした（digest `sha256:afc7e2d4…`）。repositoryにAudit固有の安全停止の記録は無い。既存のSearch G07 socket停止等は、他担当の事項として変更しない。
- 次のexact action：再reviewの指摘を反映して設計を確定する → `crates/audit-core` と `spec/telemetry` をTDDで実装する（単位A）→ 独立review → PR98 exact-head CI。

# Audit Infrastructure v1：実行状況

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

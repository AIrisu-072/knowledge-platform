# G08の合成PostgreSQL検証結果：2026-10-04

## 対象と結果

公開PR #40のheadは `0610b49327cd3c1c37e385281f087423c27b5638`、treeは `0fe596fde7d9d2fe6e5f174bf4be0f189ed55830` です。GitHub Actionsの実checkoutはmerge commit `5277cdceccf673d315a243177ed38df562317d3e` で、treeが一致しています。

14:20 UTCまでに、[CI 37207767801](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37207767801)の10ジョブ、[DSI PoC 37207767679](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37207767679)、[Sandbox 37207767796](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37207767796)の終端SUCCESSを確認しました。後続のsource変更の資格を、このheadの結果から推定しません。

[合成DBの実行ジョブ](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37207767801/job/111452453829)は、Ubuntu 24.04、Rust 1.98.1、公式PostgreSQL 18.6で次を実行しました。使い捨ての合成データだけを使用しています。

- G07 `last_claim_kill9_restart_reaps_unknown_once`：1件成功
- G07 `four_processes_disjoint_claims_and_recover_expired`：1件成功
- G08 `delivery_role_cannot_mutate_document_or_audit`：成功。専用の非所有者ロールで実store SQLを実行し、配送列以外の変更、Document/Auditの参照・変更などの拒否を確認
- G08 `invalid_trace_is_ignored_without_log_leak`：純粋試験1件が成功（DB接続なし）。無効なtraceparentの除外と、正しいtraceparentに付いた無効なtracestateの破棄を確認
- G08 `metrics_do_not_contain_high_cardinality_or_payload`：成功。実キューの状態と、有限ラベルに本文や識別子を入れない境界を確認
- 純粋・回帰：lib 7件、admission 5件、lifecycle 35件、observation 9件の計56件が成功

## 適用範囲と残る作業

[実装前の範囲・判断・ローカル検証](p6-g08-observe-preparation-20261004.md)を保持します。そこにある実DB未実施という記述は、記録時点の状態です。G08の実DB2ケース・トレース純粋1ケースと通常CIの現在の結果は本書が追加する事実であり、原文同義の翻訳や原設計の再承認ではありません。

commit-to-ack遅延は生成側の真のcommit時刻がないため未計測です。イベント時刻や0で代用していません。OTLPの外部配備・本番データでの実測は含みません。G07の接続前の実DBケースを時系列上のREDとして取得したとも主張しません。以前のローカルUnixソケット拒否は停止記録のまま保持し、その場でのDB実行を再試行していません。

次はP7のSource登録・leaseを既存の単一PostgreSQL台帳へ接続し、独立レビューと別の正確なheadで検証します。世代の保存・公開・pin・GC、本文索引、検索API、Source間連携と最終縦断の受入は未完了です。Domain migration番号9はDocument/Organization側と衝突しており、統合時には適用済み履歴を分類した互換手順と試験が必要です。既存ledgerやSQL checksumを、この記録のために変更していません。

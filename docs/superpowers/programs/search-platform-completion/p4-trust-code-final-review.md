# P4-02 trusted scope / visible catalog — final narrow code review

- 判定: **GO — P4-02 の typed trust seam に限る**。`p4-trust-code-recheck.md` の新規 P2（catalog 更新中に無関係な Source まで失敗）は閉じた。旧 P2 3件も、この seam で確認された修正を維持している。P4 全体、P4-03 以降の routing/disclosure、P5 の四 route、P7 の実 DB 永続性を受け入れる判定ではない。
- 基準: `spec/data/transaction-consistency-requirements-v0.md` SD-T8、`p4-remote-design-revision-1.md` §2、凍結 `p4-remote-plan.md` P4-02、`p4-trust-code-recheck.md`。branch `feat/search-platform-completion-core`、HEAD `80a47960d025e4dfdea1eacade28b15d218725ff`。対象ソースは未 commit。
- 開始・終了時の SHA-256: `remote_registration.rs` `806586bce63909f8d027386ebd24a240e493ae7de9072c8f223deb6584422a7f`、`scoped_catalog_contract.rs` `6f73cf924c01292ec3f4077e595c3ed59ffe5811f24a34b1d0e6db03a3546e07`。関連する `scoped.rs` は `3f45ee6dca805d1491195df7fbcc32bef62e163ca02aa0e7e05005ad89c695a6` で前回 recheck と同一。

## 最終 race 判定

1. `TrustedVisibleRegistry::visible_sources` は catalog snapshot の A と、`bind_source` が現行 catalog から発行した scope の registration/visibility revision が異なる場合、A だけを `continue` する (`remote_registration.rs:676-700`)。続く B は通常どおり bind/current/activation を検査する。決定的な `catalog_revision_change_during_bind_excludes_only_changed_source` は A=505 を v1→v2 に進め、B=506 の v1 だけを返すことを確認する (`scoped_catalog_contract.rs:478-577`)。`CheckedSourceVisibilityAdapter::bind_source` も現行 catalog/ledger に対して activation を確認して scope を発行する (`scoped.rs:463-499`)。したがって snapshot と現行 revision の競合を構造的不一致として全体失敗にしない。
2. actor/source の構造的不一致は revision 比較より前に `InvalidRequest("trusted scope unavailable")` を返す (`remote_registration.rs:689-693`)。他 actor または他 Source の scope を返す adapter を使った回帰が両方を検査する (`scoped_catalog_contract.rs:580-659`)。
3. visibility port の `bind_source`/`current` error は `?` で全体へ伝播し、個別 `Denied`/`Unknown` の除外と混同しない (`remote_registration.rs:682-685,701-702`)。`current` の infrastructure error が catalog 全体の `OperationFailed` になる回帰がある (`scoped_catalog_contract.rs:661-734`)。前回の A 取消競合で B を維持する回帰も残る (`scoped_catalog_contract.rs:407-476`)。

前回確認した ledger port 必須化、削除/再登録時の activation 失効、個別 denial の除外は `remote_registration.rs:480-637`、`scoped.rs:463-529,816-895` と対応回帰で維持されている。ただし `SourceRegistrationLedgerPort` の production 実 DB adapter、原子的 reconcile/current、再起動・複数 replica の永続所有権は P7 の独立 gate のままである。

## 独立 verification と限界

- 既存の `target/debug/deps/scoped_catalog_contract-df445aec2bfc3221 --nocapture`: **13 passed / 0 failed**。上記 race、構造的不一致、infrastructure error の 3 回帰を含む。バイナリ SHA-256 は `53997facc2355612cf4ef0bc0599c5bd4c4308652bbfb92829cb1fc9edaf8e8b`。
- ファイル更新時刻は source `17:55:35`、test `18:00:58`、バイナリ `18:03:41` (2026-09-30)。この順序と test list は現入力からのビルドと整合するが、バイナリ自体に入力 SHA の証明はない。ビルド枠を使用中のため、本 reviewer は Cargo build、strict Clippy、fmt を再実行していない。修正 worker の fresh GREEN/strict Clippy/fmt 報告は別 receipt として扱う。
- 最終判定は上記 exact source hash に対する静的レビューと既存バイナリ実行に限る。Source 変更後は再確認する。四 mode の E2E、P5 の source-neutral API、P7 storage、full CI、merge/deploy は未検証。

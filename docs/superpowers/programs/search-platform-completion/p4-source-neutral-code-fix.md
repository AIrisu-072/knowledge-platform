# P4 source-neutral catalog NO-GO 修正 receipt

- 状態: **限定修正実装・焦点検証済み、独立再監査待ち**。`feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` の未 commit worktree。入力は `p4-source-neutral-code-review.md` の二件、`p4-source-neutral-implementation-amendment.md`、P5 改訂 2、P7 改訂 1。
- F1: `SourceRegistrationCatalog` の通常公開 `try_new_synthetic`、`try_new_synthetic_with_ledger`、`replace_synthetic_checked` と catalog 内の synthetic state を削除した。legacy 13 件は `tests/support/source_catalog.rs` の test-owned host inventory が全 tenant の Document/Remote namespace snapshot を publish し、`CompleteDesiredRegistrations::capture` → 正規 `try_new` / `replace_checked` を通す。`SyntheticRegistrationLedger` の trait 実装は契約 fixture であり、物理 durability の証拠ではない。
- F2: `TrustedVisibleRegistry::visible_sources` の bind 後 actor/Source 構造不一致を固定・ID-free `SearchError::OperationFailed("trusted scope unavailable")` に変更した。Source 固有の stale revision/activation は除外、visibility infrastructure error は全体失敗のまま。

## RED → GREEN

環境は `CARGO_INCREMENTAL=0`、`CARGO_PROFILE_TEST_DEBUG=0`、`CARGO_BUILD_JOBS=2`、`--locked`。production 修正前に次を実測した。

| 検証 | RED | 修正後 |
| --- | --- | --- |
| `cargo test -p search-application --test scoped_catalog_contract structural_actor_or_source_mismatch_remains_an_error -- --exact` | `OperationFailed` 期待で 0/1、exit 101 | legacy 全体 13/13、exit 0 |
| `cargo test -p search-application --test source_neutral_catalog_contract structural_mismatch_or_infrastructure_error_fails_whole_snapshot -- --exact` | 同期待で 0/1、exit 101 | source-neutral 全体 20/20、exit 0 |
| `cargo test -p search-application --doc source_registration::SourceRegistrationCatalog -- --nocapture` | 旧公開 Vec 入口がコンパイルできるため compile-fail guard 0/3、exit 101 | 三入口が E0599 で拒否され 3/3、exit 0 |

`port_contract` 4/4、所有四ファイルの `rustfmt --edition 2024 --check`、`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2 cargo clippy -p search-application --all-targets --locked -- -D warnings` は exit 0。`partial_foreign_capture_cannot_replace_catalog_or_tombstone_other_tenant` は、正規 `replace_checked` に別 host の部分 desired を渡しても ledger state と他 tenant の current activation が変わらないことを確認した。旧 API 参照は production では消え、三つの compile-fail 文書テストにだけ残る。

## 固定入力と範囲

| ファイル | SHA-256 |
| --- | --- |
| `crates/search-application/src/source_registration.rs` | `60975ee86938f3a3f59b18d0945ca10385eb5374419bd5c0d51cbd88963cc17f` |
| `crates/search-application/tests/scoped_catalog_contract.rs` | `459254b8b13d31ec7326182d66c8df816e6de478b611107aa61a8547dae6aeee` |
| `crates/search-application/tests/source_neutral_catalog_contract.rs` | `edd54ac9e541b5c542a8427069320841190579d362cee8e6dd357da52038883b` |
| `crates/search-application/tests/support/source_catalog.rs` | `68e9604fedf97c244f29a230182084ea45a7170cc3eda0fb4428500fc165469f` |

準備中に `/tmp/p4-source-registration-candidate.rs` と `/tmp/p4-source-neutral-production-fix.patch` だけを書こうとした Python command は `INDEX-MD GUARD` が `specs/.index.md` 変更の可能性として実行前 DENY。保護対象への変更も、その command による `/tmp` 書込みもない。実修正は対象 repository file への `apply_patch` で適用した。

本 receipt は P4 の union catalog と scope 分類だけを対象とする。P5 HTTP 四 route の generic 503 mapping、P7 host production factory と実 PostgreSQL global ledger/lock、physical durability は別 gate。次の exact action は固定 SHA の独立 read-only 再監査で F1/F2 の閉鎖を判定すること。

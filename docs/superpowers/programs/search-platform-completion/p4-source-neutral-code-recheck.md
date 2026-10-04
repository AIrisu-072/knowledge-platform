# P4 source-neutral trusted catalog / ledger NO-GO 修正の独立再監査

- 判定: **GO — P4-02 source-neutral typed seam の二件の NO-GO は閉鎖**。対象は `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` の未 commit worktree。P4 全体、P5 HTTP 四 route、P7 実 PostgreSQL durability / production factory の GO ではない。
- 入力: `p4-source-neutral-code-review.md` の F1/F2、`p4-source-neutral-code-fix.md`、`p4-source-neutral-implementation-amendment.md`、P5 API 改訂 2、P7 shared durable 改訂 1。レビュー中に対象 source/test の SHA-256 は変化しなかった。branch に対応する open PR は `gh pr list --head feat/search-platform-completion-core` で 0 件。

## NO-GO 二件の閉鎖

1. **F1 — 公開 Vec catalog bypass を削除。** `SourceRegistrationCatalog` の公開生成・更新 API は `try_new(ledger, &document_complete, &remote_complete)` と `replace_checked(&CompleteDesiredRegistrations)` のみ (`source_registration.rs:1114-1155`)。旧 `try_new_synthetic`、`try_new_synthetic_with_ledger`、`replace_synthetic_checked` は三つの `compile_fail` 文書例にだけ現れ、production method として存在しない (`source_registration.rs:1049-1084`)。`RemoteRegistrationCatalog` は同じ catalog の互換 alias だけで、別の Vec constructor はない (`remote_registration.rs:393-399`)。legacy 13 件の setup は `tests/support/source_catalog.rs:22-104` に移り、test-owned host に Document/Remote の namespace inventory を publish し、`CompleteDesiredRegistrations::capture` から正規 API を通す。production `src` にその helper は含まれない。
2. **F2 — 構造的不一致を dependency error に分類。** `TrustedVisibleRegistry::visible_sources` は bind 後の actor/Source 不一致で、固定かつ ID-free の `SearchError::OperationFailed("trusted scope unavailable")` を返す (`source_registration.rs:1297-1308`)。legacy test は actor と Source の二種で同じ文言・ID 不含を確認する (`scoped_catalog_contract.rs:643-666`)。新 test も構造的不一致と visibility infrastructure error が一覧全体の `OperationFailed` になることを確認する (`source_neutral_catalog_contract.rs:1046-1100`)。この port error を四 route 共通の generic `503 DEPENDENCY_UNAVAILABLE` に写す HTTP handler は P5 の別 gate。

## 保持された境界

- `CompleteDesiredRegistrations::capture` は host port の snapshot と canonical digest を検査する (`source_registration.rs:720-737`)。synthetic ledger は candidate と独立 host snapshot を reconcile 前と commit 境界で全 key/variant/DTO/revision/digest まで照合する (`source_registration.rs:897-929`)。任意の `HostRegistrationSnapshotPort` 実装の完全列挙は型だけで証明できないため、P7 production factory が trusted host authority を固定する必要がある。
- 一つの ledger は SourceId の tenant/kind を tombstone 後も固定し、当該 namespace のみ tombstone にする (`source_registration.rs:952-1019`)。`is_current` は保存 DTO 全体・activation・両 revision を照合する (`source_registration.rs:1029-1045`)。新 test は foreign host の部分 desired を正規 `replace_checked` に渡しても ledger state が原子的に不変で、他 tenant の activation が current のままと確認する (`source_neutral_catalog_contract.rs:573-601`)。Document/Remote 同一 SourceId は tombstone 後も拒否される (`source_neutral_catalog_contract.rs:603-620`)。
- `TrustedVisibleRegistry` は既存 actor/Source mint で Document/Remote の union catalog を列挙し、Source 固有競合を除外、port error を全体失敗とする (`source_registration.rs:1263-1336`)。更新中・unknown commit は local current gate を閉じる (`source_registration.rs:1151-1189,1210-1259`)。安定 stamp は主張せず `None` のまま (`source_registration.rs:1335`)。
- `SyntheticRegistrationLedger` は公開 trait の contract fixture であり、disk / multi-replica durability の証拠ではない (`source_registration.rs:815-827,856-863`)。P7 の concrete SQL adapter、global serial lock、host snapshot current 再照合、role/factory と実 DB named regressions は未判定。`HostRegistrationSnapshot::from_complete_host_inventory(Vec<_>)` の名称だけも全 tenant 完全性を証明しない (`source_registration.rs:675-700`)。

## 独立した fresh 検証

最初に既存 test executable を再実行して 20/20、13/13、4/4 の exit 0 を観測したが、`source_registration.rs` の mtime `2026-09-30T12:25:32Z` は既存 binary `12:24:30/40/52Z` より新しかったため、**この結果を現行 source の GREEN と数えなかった**。App 専有 Cargo slot で次を現行 source から再ビルドした。環境は `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2`、いずれも `--locked`。

| command | fresh result |
| --- | --- |
| `cargo test -p search-application --locked --test source_neutral_catalog_contract --test scoped_catalog_contract --test port_contract` | `search-application` を再コンパイル、source-neutral **20/20**、legacy scoped **13/13**、port **4/4**、exit 0 |
| `cargo test -p search-application --locked --doc source_registration::SourceRegistrationCatalog -- --nocapture` | 旧 API 三つが各々 E0599 で拒否される `compile_fail` **3/3**、exit 0 |
| `rustfmt --edition 2024 --check`（修正対象 source/test 四ファイル） | exit 0 |

再生成 binary の mtime は source/test より後の `2026-09-30T12:50:42Z`。SHA-256 は `source_neutral_catalog_contract` `08cb936c3d9a9016d102a495527d5343df3d624ad073f457d8e43a3e0dbf9420`、`scoped_catalog_contract` `87e6adc4fd719846a0a57b98b444b4a5b5a4e935d684509ae5b95988f2d3f868`、`port_contract` `ea91a7e0df06be630c5b2f0174fdcf9a4a45cb5e022b25416c678ddcb3ebc9f1`。対象ファイルを build 前後に SHA-256 で照合した。writer receipt の strict App Clippy exit 0 は参照したが、この再監査では再実行していない。

| 対象ファイル | 開始 = 終了 SHA-256 |
| --- | --- |
| `crates/search-application/src/source_registration.rs` | `60975ee86938f3a3f59b18d0945ca10385eb5374419bd5c0d51cbd88963cc17f` |
| `crates/search-application/src/remote_registration.rs` | `a0d8ea5765331cad46a7203c9eeafa55f0859a56d07bd4b334ae560a776968c0` |
| `crates/search-application/src/scoped.rs` | `5b5ae37f9eb39c51caa460ba235ee4d2b3c43d50a9475bc227fcd27a6102d581` |
| `crates/search-application/src/lib.rs` | `3ae6887b4630fb41be9254c384e256889a62605d609cf04f02175d8021d73d14` |
| `crates/search-application/tests/source_neutral_catalog_contract.rs` | `edd54ac9e541b5c542a8427069320841190579d362cee8e6dd357da52038883b` |
| `crates/search-application/tests/scoped_catalog_contract.rs` | `459254b8b13d31ec7326182d66c8df816e6de478b611107aa61a8547dae6aeee` |
| `crates/search-application/tests/support/source_catalog.rs` | `68e9604fedf97c244f29a230182084ea45a7170cc3eda0fb4428500fc165469f` |
| `crates/search-application/tests/port_contract.rs` | `82594997f5e68bd19611b5ffa161d7ad038cf42c9c7c94502a78dc853615c82b` |
| `p4-source-neutral-implementation-amendment.md` | `2c737192787b55922c4fb49ee9cda06931753e4a1b8e78881aa708c0bc7ca204` |
| `p5-api-contract-revision-2.md` | `81c8be90375bd2461fa8179ef7d5fdb0705c1b02431dee918d99ac26372901d9` |
| `p7-shared-durable-revision-1.md` | `2e2f1a24972f2020278cfe7fe37c1726ae280e936685ef74299c1c5c2a8641fe` |

**次の exact action:** P5 HTTP writer は四 route でこの port の error を generic 503 に写し、安全な SourceItem projection と final gate を別に検証する。P7-02 writer は同じ port に concrete SQL ledger と trusted production factory を接続し、named real-PG regressions を独立 gate に掛ける。

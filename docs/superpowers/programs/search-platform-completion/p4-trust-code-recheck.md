# P4-02 trusted scope / visible catalog — repaired code recheck

- 判定: **NO-GO（新規 P2 1件）**。旧レビューの P2 3件は P4-02 の typed trust seam で閉じた。P7 の実 PostgreSQL ownership ledger は別の storage gate として未実装・未検証であり、P4-02 に物理永続性を認定しない。本判定は P4-02 の現コードに限定し、P4 全体・P5 transport・P7 runtime の受入ではない。
- 基準: `spec/data/transaction-consistency-requirements-v0.md:1086-1092` (SD-T8)、`p4-remote-design-revision-1.md:66-70`、凍結 `p4-remote-plan.md` P4-02、旧 `p4-trust-code-review.md:7-13`。branch `feat/search-platform-completion-core`、HEAD `80a47960d025e4dfdea1eacade28b15d218725ff`、対象ソースは未 commit。
- SHA-256: `scoped.rs` `3f45ee6dca805d1491195df7fbcc32bef62e163ca02aa0e7e05005ad89c695a6`、`remote_registration.rs` `4fcc6e4f755a441f8a7a670e8bbd1371aec434f5934f1bf2c0ddbc9997bcf93b`、`scoped_catalog_contract.rs` `61844ffe59d09eebe5bf1c46a95d1bd21e9ad762e7548cf41d437f712325857d`、`lib.rs` `26c23d5f331fb50d06651eb2b0f4b9c0e308cf134673745235dd772f4ce372fa`。

## Blocking finding

1. **[P2] A の registration revision 更新競合が、無関係な可視 Source B の列挙を失敗させる。** `TrustedVisibleRegistry::visible_sources` は `catalog.values()` の snapshot を取った後に各 Source を bind する (`remote_registration.rs:676-685`)。A を v1→v2 に更新し、host visibility grant も v2 へ進めた時、`CheckedSourceVisibilityAdapter::bind_source` は現行 catalog/ledger から v2 scope を正常発行する (`scoped.rs:473-497`)。しかし registry は A の snapshot v1 と scope v2 の revision 不一致を **全体の `InvalidRequest`** に変換する (`remote_registration.rs:694-700`)。A が B より先に並ぶ場合、B の bind/current へ進まない。合成 adapter でも同じ順序で再現できる。これは個別 Source の現行性喪失をその Source の除外として扱う §2/SD-T8 と、旧 P2-3 の独立 Source 保護の意図に反する。actor/source/tenant の構造的な不一致はエラーのまま、snapshot と現行 revision/activation の競合だけを A の除外にし、B を返す決定的回帰テストが必要。既存 10件は visibility の `Denied` 競合を試すが、この catalog 更新競合を試さない (`scoped_catalog_contract.rs:407-476`)。

## 旧 P2 3件の再確認

1. **過去 owner:** production `RemoteRegistrationCatalog::try_new` は `Arc<dyn SourceRegistrationLedgerPort>` を必須とし、非同期 `reconcile` の全 Source receipt を確認する (`remote_registration.rs:332-349,480-495`)。同じ synthetic ledger を共有した catalog 再生成で tenant 変更を拒否する回帰がある (`scoped_catalog_contract.rs:308-326`)。synthetic の共有 instance は再起動を模擬するだけで物理 durable ではない。P7 は全 tenant/Document/Remote 共通 ledger、既存 generation owner 検査、原子的 reconcile/current、tombstone、deployment revision と複数 replica の同一 desired set を実 DB で証明する必要がある (`p7-shared-durable-design.md:18-22`)。現行 crate に実 DB adapter はない。
2. **削除後 old scope:** `AuthorizedSourceScope.registration_activation` は private で adapter のみが発行する (`scoped.rs:175-203,479-497,837-855`)。catalog 削除で synthetic ledger の activation が進み、再追加にも旧 revision を許さない (`remote_registration.rs:380-445`)。Checked/Synthetic の `current` は grant と catalog/ledger の現行 activation を共に照合する (`scoped.rs:501-529,859-895`)。削除、再登録、新 scope と旧 scope の回帰がある (`scoped_catalog_contract.rs:328-405`)。
3. **個別 Denied/Unknown:** `TrustedVisibleRegistry` は `bind_source=None` と `current != Allowed` をその Source だけ除外し、actor の current を列挙前後に確認する (`remote_registration.rs:675-720`)。A の bind 後取消で B を維持する回帰がある (`scoped_catalog_contract.rs:407-476`)。visibility/ledger の port error は `?` で全体を fail closed にし、個別 denial と混同していない。`BoxFuture` は `Send` を要求し、対象の非同期実装は strict Clippy までコンパイルされた (`ports.rs:32`)。

## Verification と境界

- 現 hash で `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 cargo test -p search-application --locked --test scoped_catalog_contract`: **10 passed / 0 failed**。旧レビューの 7件に修正回帰 3件を追加した現 GREEN を独立再実行した。修正前 RED のコマンド出力は本レビューでは再実行・確認していない。
- 同環境で `cargo clippy -p search-application --locked --test scoped_catalog_contract -- -D warnings`: **PASS**。開始・終了時の対象 SHA-256 は一致。対象 source は未 commit で、full CI・実 PostgreSQL・E2E は未実行。
- `try_new` / `replace_checked` の `await` 時点に lock guard はなく、port `BoxFuture` は `Send` である (`remote_registration.rs:482-495,538-568`, `ports.rs:32`)。`replace_checked` は ledger reconcile が error、commit 応答 unknown、または receipt key 不一致を返しても local projection を変更せず、更新中の `current_activation` は拒否する。正常 receipt 後にだけ local を入れ替える (`remote_registration.rs:538-568,600-637`)。commit 済みで応答だけ失われた場合、変更済み Source の stale local scope は `ledger.is_current` との不一致で fail closed となる。ledger 自体の原子的 rollback と durable `is_current` は port 実装の P7 gate に依存し、実 DB transaction/cancel/restart fault は未確認。
- 次の exact action: 上記 A 更新競合の RED を追加し、revision 競合を個別除外へ修正する。同じ 10件と追加回帰、対象 strict Clippy を再実行し、別 reviewer が exact hash を再確認する。その後に P4-02 typed port を再判定し、P7 storage gate は独立に保持する。

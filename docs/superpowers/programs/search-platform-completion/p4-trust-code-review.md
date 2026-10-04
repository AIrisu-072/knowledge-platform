# P4-02 trusted scope / visible catalog — independent code review

- 判定: **NO-GO（P2 3件）**。対象は P4-02 の実コードのみ。P4 全体、P4-03 routing、P4-07 read lease、P4-13 service、P7 runtime の受入判定ではない。
- 照合基準: `spec/data/transaction-consistency-requirements-v0.md` SD-T8、`p4-remote-design-revision-1.md` §2、frozen `p4-remote-plan.md` P4-02。確認 branch は `feat/search-platform-completion-core`、基点 HEAD は `80a47960d025e4dfdea1eacade28b15d218725ff`。対象コードは未 commit。
- 対象 SHA-256: `scoped.rs` `8e36a9fe70c5faf4c131554dc7a63b7ef292a0f9e9eed1320db31a903da1c9b2`、`remote_registration.rs` `97e3a4bcf1b017936b3e0d63cfad8f35a4977219620dc7bde2c63b27f6020cd2`、`lib.rs` `26c23d5f331fb50d06651eb2b0f4b9c0e308cf134673745235dd772f4ce372fa`、`scoped_catalog_contract.rs` `c50939bcbcb853303105df4b03d702dfe5898a33b20136230ac80df1ff7dcbb8`。

## Blocking findings

1. **[P2] 再起動後に `SourceId` の tenant 所有権を失う。** `RemoteRegistrationCatalog::try_new` は受け取った現行登録から `history` を新規生成するだけ（`remote_registration.rs:312-326`）。`replace_checked` はそのインスタンスの履歴だけと比較する（同:343-360）。再現: tenant A に SourceId X を登録し、削除または再起動後、tenant B/X だけで新しい `try_new` を呼ぶと成功する。既存の `(SourceId,generation)` は tenant を含まないため、SD-T8 と設計 §2/P7 startup が禁止する再割当てを拒否できない。P4-02 単体の同時登録重複拒否は機能するが、P7 で永続 Source 所有台帳と既存 generation key を照合し、起動・更新を一つの原子的な所有権判定にするまで global uniqueness の受入は不可。少なくとも再起動を挟む回帰を追加する。

2. **[P2] 登録削除が発行済み `AuthorizedSourceScope` の current 判定を失効させない。** `RemoteRegistrationCatalog::replace_checked(vec![])` は現行 map を空にするだけ（`remote_registration.rs:343-361`）。`SyntheticVisibilityAdapter::current` と `CheckedSourceVisibilityAdapter::current` は visibility grant/revision のみを参照し、catalog の現行登録を確認しない（`scoped.rs:472-490,798-820`）。さらに同じ登録/revision の再追加は許可される（`remote_registration.rs:348-355`）。再現: Source X の可視 scope を取得、catalog から X を削除、grant を維持したまま `visibility.current(&old_scope)` を呼ぶと `Allowed`。同じ revision の X を再追加しても old scope を区別できない。SD-T8 の「各 read/write で現行 registration/revision を検証」と取消後の一括除去を満たすには、scope current gate に現行 catalog と単調な削除 tombstone/revision を含め、削除・再登録で旧 scope を再利用できないようにする。P4-07/P5/P7 の利用側にも同じ gate を必須にする。

3. **[P2] ある Source の visibility 取消競合で、別の可視 Source まで列挙に失敗する。** `TrustedVisibleRegistry::visible_sources` は `bind_source` 後の `current` が `Denied`/`Unknown` なら Source を除外せず、全体に `InvalidRequest` を返す（`remote_registration.rs:424-440`）。再現: A と B を登録し、A の bind 直後に visibility を取り消して A の `current` を `Denied` にする。B が許可されていても `visible_sources` は Err となる。設計 §2 と P7 runtime draft は個別 Source の Denied/Unknown を集合から除外し、registry 自体の障害のみ全入口を fail closed とする。個別の denial は `continue` し、actor/registry の失効やエラーとは区別する race test が必要。

## 確認できた点と残る境界

- `TrustedSearchScope`、`TrustedDiscoveryBinding`、`AuthorizedSourceScope` の field は private。production 向け `CheckedAuthorityAdapter` は host-configured `VerifiedActorResolverPort` から raw handle を検証して mint し、`verify_discovery_binding` は handle/evaluation/current actor を registry より前に照合する（`scoped.rs:111-179,311-404,493-560`）。`AccessContextHandle` の `Debug` は raw value を隠す（同:68-82）。`VerifiedActorDescriptor::new` 自体は public だが、HTTP caller/provider が resolver 実装や composition root を選べない構成なら scope mint への直通路ではない。
- `RemoteSourceRegistration` は private fields と server-config 変換を使用し、provider response からの変換 API はない。catalog 内の重複 ID と tenant 変更は拒否し、`replace_checked` は検証後に現行 map を入れ替える（`remote_registration.rs:140-226,320-361`）。`RegisteredEndpoint` は `http` も表現可能であり、HTTPS・SSRF 制約は P4 HTTP transport の production adapter で強制する境界。P4-02 のテスト結果で transport が安全とは主張しない。
- `SyntheticAuthorityAdapter::issue_verified_identity` と `SyntheticVisibilityAdapter::grant` は production build でも public（`scoped.rs:575-649,715-769`）。設計が許す synthetic harness 用 issuer として確認した。P7 composition root は production でこれらを選択できないことを起動時に保証し、host verifier/visibility だけを配線する必要がある。
- Required/Preferred ID の交差、ID を含まない gap、途中失効時の candidate/Claim/rank/trace/count 除去は P4-03/P4-13 以降の対象。このコードにはまだ routing/service への接続がなく、可視集合の安全性をもって外部結果の非漏洩を認定しない。

## Verification

- `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 cargo test -p search-application --locked --test scoped_catalog_contract`: **7 passed / 0 failed**。`search-core` の別作業由来の dead-code warning 2件あり。対象 path の tracked diff に対する `git diff --check` は異常なし（新規ファイルは対象外）。
- この 7件は同一インスタンスの重複、foreign handle、revision、可視投影を検査する。上記の再起動を挟む所有権、catalog 削除後の old scope、per-Source denial race は未検査。修正後にこれらの focused regression と同じ 7件を再実行すること。

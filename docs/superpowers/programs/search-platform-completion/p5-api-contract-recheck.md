# P5 Full API — source-neutral catalog / HTTP wire 契約の独立再照合

- 判定: **GO — P1〜P4 と合成した P5 Full API 設計契約**。先行 `p5-api-contract-review.md` の P1 2件・P2 1件は改訂 2 で閉じた。新たな P1/P2 の設計指摘はない。P5 Design Freeze の precedence 記録へ進める判定であり、P4-02 code、P7 ledger、Search 専用 `spec/`/OpenAPI、四 route、実 HTTP、hosted gate の受入ではない。
- 対象: branch `feat/search-platform-completion-core`、基点 HEAD `80a47960d025e4dfdea1eacade28b15d218725ff`。主対象 `p5-api-contract-revision-2.md` SHA-256 `81c8be90375bd2461fa8179ef7d5fdb0705c1b02431dee918d99ac26372901d9`。本レビュー以外のファイルは変更しない。

## 先行三件の再判定

1. **共通 local+remote 可視 catalog: 閉じた。** 改訂 2 `:13-59` は `DocumentSourceRegistration` と既存 `RemoteSourceRegistration` の sum type、`VisibleSourceRegistration` の同一 `AuthorizedSourceScope`、`VisibleCatalogSnapshot`、`ScopedSourceRegistryPort::visible_sources` の返却型を明示する。`BoxFuture<'a,T>` は現行 `ports.rs:32` で `Result<T,SearchError>` を内包するため、`Err` による fail closed の記述とも整合する。完全な actor-visible Document+Remote 列挙だけが成功し、個別 `Denied`/`Unknown`・当該 Source の revision/activation 競合だけを除外する。actor/registry/ledger/visibility error、重複 ID、構造的不一致は四 route 共通の generic 503 であり、途中一覧や隠れた Source の障害件数を出さない（`:57-59`）。`SourcePage` の capability は登録済み・実接続済みの能力だけから安全に射影する（`:57`、照合追補 `:44-46`）。stamp がないときに `SourcePage` は部分成功を持たないので、単一 page に全件収まる場合だけ 200 を返せる。複数 page が必要なら cursor なしの切捨て 200 は不可であり、後続 plan/schema/test にこの派生条件を明記する。
2. **ownership/current/namespace と mint の一元化: 閉じた。** 改訂 2 `:63-87` は `CompleteDesiredRegistrations` を namespace ごとの全 tenant 完全集合とし、Document/Remote の reconcile 非干渉、同一 revision の異なる digest・旧 revision の拒否、二 namespace 初期 reconcile 前の起動拒否を規定する。同じ durable ledger が全 tenant/kind の過去 SourceId owner と既存 `(SourceId,generation)`、登録全体、両 revision、単調な activation/tombstone を照合し、削除後の旧 scope・tenant/kind 再割当てを拒む。`is_current` は現行 port 名に揃った。`CheckedAuthorityAdapter` / `CheckedSourceVisibilityAdapter` の mint、P4 `TrustedSearchScope` / `AuthorizedSourceScope` / current gate を両 variant で共有し、別の actor/Source 許可集合や Discovery loop を作らない（`:9,87`）。現行 `scoped.rs:206-240,277-283,449-529` と `remote_registration.rs:332-349,468-495,649-720` は remote-only であり、この新型や durable ledger の実装済み証拠ではない。改訂 2 `:63,133` の P4-02 implementation amendment と独立 code review を **P5 code の前提**として保持する。P7 の PostgreSQL 案と新しい repair draft は未受入・未接続である。
3. **401 challenge と期限 wire: 閉じた。** 改訂 2 `:91-107` は実 transport の登録済み security scheme、認証 adapter、operation 別の適用可能な `WWW-Authenticate` challenge を同じ binding に結び、空・不正・未配線 challenge では route 起動を拒否する。未認証 401、認証済み操作拒否 403、resolver 障害 503 を区別し、同じ opaque session で四 route を通す trusted local server fixture と OpenAPI/handler/header parity を要求する。これは [RFC 9110 §15.5.2](https://www.rfc-editor.org/rfc/rfc9110.html#section-15.5.2) の 401 必須 header に合う。改訂 2 `:111-127` は内部 final gate/local read/projection/serialization deadline を `503 SERVICE_UNAVAILABLE`、実 gateway/proxy の必要 upstream wait timeout のみ `504 UPSTREAM_TIMEOUT` に分ける。[RFC 9110 §15.6.4–5](https://www.rfc-editor.org/rfc/rfc9110.html#section-15.6.5) の 504 範囲を内部処理へ広げない。期限内に安全な独立 DTO と全 final gate が確定した場合だけ 200 partial、header 送出後は status 差替えをせず接続と lease を閉じる。

## 継承した設計境界

- `about:blank`、固定 HTTP reason `title`、追加 `code`、status/body/header parity、全 Problem の `private, no-store` / `nosniff` は改訂 2 `:127` と照合追補 `:79-103` で一貫する。[RFC 9457 §4.2.1](https://www.rfc-editor.org/rfc/rfc9457.html#section-4.2.1) では `about:blank` は HTTP status を超える追加意味を持たず、P5 の詳細分岐は宣言済み extension `code` が担う。既存 Document HTTP の URN 契約はこの判定で変更しない。
- P1 Archive binding ruling の exact SHA を改訂 2 `:5,131` が継承する。異種 leaf の Archive part は part-wide `archive_inner_format=None` を許し、各 Unit の `Some(leaf)`、member chain、trusted plan、共通 profile/outer reader、親 Version/Part/raw/current Read を検証する。P1 full body の positive/negative/Partial coverage は照合追補 `:52` のまま。
- P3 typed n-ary relation は全 participant・metadata の同一 Source/generation/current grant、不可視時の派生 item/evidence/count/trace 一括除去を維持する（照合追補 `:54`、P3 freeze `:9-10`）。P4 の二 lease、五 retention mode、`NO_RETENTION` の cursor/store 禁止と送出完了/error/disconnect/cancel/deadline close は照合追補 `:58-69` を維持する。

## Exact input と確認範囲

下記 SHA-256 は再照合の開始時と文書作成後に再計算した。`p7-shared-durable-design.md` は draft 入力であり、`p7-shared-durable-revision-1.md` は作業中の未受入 draft として現物 SHA のみ確認した。

| 入力 | 開始時 SHA-256 | 終了時 |
| --- | --- | --- |
| P5 contract revision 2 | `81c8be90375bd2461fa8179ef7d5fdb0705c1b02431dee918d99ac26372901d9` | 同一 |
| P5 reconciliation | `a17920c81928c638ea11204f6dce5e62a0c72d52a286431c70d743dd62d8c12e` | 同一 |
| P5 NO-GO review | `21df587285a0a7fb08d916ea0fd8524866a02272b4e34d932ac00fd4bb0fb3dc` | 同一 |
| P5 design revision 1 / architecture GO | `f5c43d20a143a00d5dfc92bc60ea29735f9a0c884f00deae024f6e2971f9887b` / `a9ab8e9224e100d376b6b8715eaa8cdf54bf2a10825e65c56e36243c2a47e20f` | 同一 |
| P1 composed / minimal freeze / Archive ruling | `205c5a5ff68843e073da8d87b825a55078dbdb66bd985f22d2044eb888fd406d` / `0fe9ceb84472633d06c52d84f2f62c86bc783f880751c935a7d773b1c8fdfc84` / `eadaa11a8c02953f48e533c53d549e9b26f346dc862231d3d7622a9ee615599c` | 同一 |
| P3 freeze | `773be941a35a2984495614ea41db74f70d325f4c034fe576d4d106b4af0e6ca3` | 同一 |
| P4 freeze / plan | `7b38cae0e4057556970a36c9df4752348c0ad1d7c4cac916eb794275d35bd5c5` / `a713a6f0d2f5f1d0066576dada09f4987e54bccf5c7df2330fa89f565bb31da0` | 同一 |
| P7 shared durable draft | `99a989e30ec77ea5f79915a3db22ecd31b910decbabc825c2ae8bca6f1041a3b` | 同一 |
| P7 shared durable revision 1 (未受入 draft) | `2e2f1a24972f2020278cfe7fe37c1726ae280e936685ef74299c1c5c2a8641fe` | 同一 |
| `scoped.rs` / `remote_registration.rs` | `3f45ee6dca805d1491195df7fbcc32bef62e163ca02aa0e7e05005ad89c695a6` / `806586bce63909f8d027386ebd24a240e493ae7de9072c8f223deb6584422a7f` | 同一 |

次の exact action: P5 design precedence と freeze に本書および改訂 2 の exact SHA を取り込む。P5 code 前に P4-02 の source-neutral implementation amendment、union catalog/namespace ledger の実装・独立 review を完了する。Search 専用 `spec/`、四 operation OpenAPI、`p5-api-plan.md` に source-neutral 型、401 header、503/504 registry、SourcePage 無継続時、実 security scheme と全 schema/header parity を反映する。公開資格は P1 full body/Archive、P3 participant、P4 二 lease と remote TCP、Document 実 DB、trusted fixture、四 route の実 HTTP、exact-head hosted gate の別証拠で判定する。

確認方法: 現行型・port と関連 freeze/plan/先行レビューを静的に照合し、RFC 9110/9457 の公式文面を確認した。build、CI、実 HTTP 試験はこの設計レビューでは実施していない。

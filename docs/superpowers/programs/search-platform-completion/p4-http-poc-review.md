# P4-15 HTTP client PoC 独立監査 — 2026-09-30

## 判定

**GO（HTTP library selection と P4-16 への依存昇格のみ）**。`reqwest = "=0.13.5"`、`default-features = false`、`rustls` と `gzip` の組合せについて、隔離 PoC に transport または dependency の選定 blocker は見つからなかった。これは `search-source-http` の本番 transport 実装、P4 の実 TCP provider/service E2E、CI、merge/deploy の受入ではない。PoC の `GuardedTransport` をそのまま本番へコピーする判定は **NO-GO**。

判定対象は untracked の `experiments/search-http-client-poc/` であり、Git HEAD `80a47960d025e4dfdea1eacade28b15d218725ff` だけでは内容を同定できない。`Cargo.toml`、`Cargo.lock`、`deny.toml`、`src/lib.rs`、`tests/transport.rs` の path/length framed SHA-256 を独立再計算し、報告書の **`1f42e2a0d54d2dda68cdac5c7d2e1aa29539309c30dbd72b44fabaa66222bd50`** と一致した。選定表 `spec/selection/library-tool-selection-v0.md` の確認時 SHA-256 は `b12dc484fe8b596a924c06abdcce92dc39dffcff1ea562ca74c42875638656ba`。

## 根拠と再検証

| 観点 | 確認した根拠 | 判定 |
| --- | --- | --- |
| 固定 HTTPS URL | `src/lib.rs:84-140,143-174`。production constructor は HTTPS・userinfo 禁止・IP literal 禁止で、`fetch` に URL 引数はない。PoC では constructor へ渡す `&str` の信頼元は型で保証していない。 | library 機構は適合。本番は `RegisteredEndpoint` が必要。 |
| DNS、SSRF、pin、pool | `src/lib.rs:49-73,154-174,243-279`。全返却 address の port と public class を接続前に検査し、IPv4-mapped IPv6 も拒否する。fresh client、idle pool 0、`resolve_to_addrs` に検査済み address を渡す。`tests/transport.rs:240-269` は metadata/mapped の混入拒否、pin 成功、次 call の悪性再解決拒否と TCP request 数 1 を確認する。IPv4/IPv6 class は確認時の IANA special-purpose registry と矛盾しない。 | 適合。実 OS DNS の可用性や全環境の resolver 動作をこの合成試験から推定しない。 |
| TLS hostname | `src/lib.rs:172-178` は rustls と元 URL hostname を使う。`tests/transport.rs:306-354` は同一 pin address/custom test root で正しい hostname が成功し、異なる hostname が失敗する。SNI 値そのものは test server で採取していない。 | hostname 証明書検証は実 TCP/TLS で確認。P4-16 で SNI も明示観測する。 |
| proxy、redirect、retry | `src/lib.rs:164-169,188-193` は `no_proxy`、redirect none、retry never、fresh client を設定。`tests/transport.rs:157-238,523-536` は redirect target の request 数 0、環境 proxy の request 数 0、切断時送信数 1 を調べる。proxy は optional `ambient-proxy-canary` と HTTP loopback で検証され、HTTPS CONNECT/system 設定の全変種を測定したものではない。 | library API と対象試験は適合。P4-16 でも実装の連結後に回帰する。 |
| bytes、期限、cancel | `src/lib.rs:108-133,181-231` は PoC GET の保守的 request framing 見積り、`Content-Length` が見える場合の先行拒否、展開済み chunk 総量、connect/header・read・total の期限、cancel の body drop を実装。`tests/transport.rs:356-521` は巨大 header 値、gzip 展開過大、TLS/header stall、body stall、trickle、TCP EOF を確認する。gzip 自動展開時には reqwest が `Content-Length` を除去するため、展開後の実 byte 計数が決定的な制限である。 | PoC の GET/単一 call では適合。POST body、全 response header/RAM、並行数、5 秒 evaluation、JSON/page/action は未実装。 |
| 依存 gate | 独立実行の `cargo tree --locked -e features -i reqwest` は selected graph に `rustls`/`gzip` のみを示し、`system-proxy` は含まない。lock は `reqwest 0.13.5` と crates.io registry source。`cargo deny 0.20.2 ... check` は `advisories ok, bans ok, licenses ok, sources ok`、`syn 2.0.119/3.0.6` の duplicate warning 1 件。 | 適合。P4-16 root lock の新しい graph で gate を再実行する。 |

新規再実行（macOS、Rust 1.98.1、`CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2`、隔離 `CARGO_TARGET_DIR=experiments/search-http-client-poc/target`）:

- `cargo test --manifest-path experiments/search-http-client-poc/Cargo.toml --locked --test transport all_a_aaaa_checked_and_pinned_against_rebind -- --exact`: **1/1 PASS**、8 filtered out。fresh build 1m03s。
- 同条件で `hostname_tls_validation_preserved -- --exact`: **1/1 PASS**、8 filtered out。
- `cargo deny --manifest-path experiments/search-http-client-poc/Cargo.toml --config experiments/search-http-client-poc/deny.toml check`: **exit 0**、上記 4 gate PASS、duplicate warning 1 件。

PoC 報告 `report.md:46-53` の RED/GREEN、全 test、feature canary、Clippy の記録は閲覧したが、この監査で再実行したのは上の TCP 2 件と依存 gate だけ。reqwest の [ClientBuilder 0.13.5](https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html) は DNS override、proxy・redirect・retry・timeout と gzip の挙動を文書化しており、[retry::never](https://docs.rs/reqwest/0.13.5/reqwest/retry/fn.never.html) も確認した。address class は [IANA IPv4](https://www.iana.org/assignments/iana-ipv4-special-registry) と [IANA IPv6](https://www.iana.org/assignments/iana-ipv6-special-registry) を参照した。

## 指摘と P4-16 昇格条件

1. **P2／本番へのコード流用 gate:** PoC の `new_loopback_for_test` は公開 API で、`http` と custom trust root を受け入れる (`src/lib.rs:90-106,126,175-177`)。隔離 PoC では意図した試験 hook だが、本番 crate では plan P4-16 の `synthetic-loopback-test-only` feature にのみ閉じ、release build でその feature を compile error とし、production config から構成不能にする。PoC の raw endpoint `&str` と可変 `Limits` も、trusted registration と上限固定を型・constructor で強制する。
2. **P2／本番 transport 受入 gate:** PoC は固定 GET だけ (`src/lib.rs:143-183`)。P4-16 の `RegisteredPath` と bounded POST body、exact origin/port/path、response header・並行 RAM・評価全体の上限、SNI 観測、no-retry/no-spool、失敗時の低 cardinality gap 変換を独立試験する。URL/authority を provider JSON、caller、HTTP header に委ねない。`NO_RETENTION` の lease と実 service E2E は P4-17 以降の別 gate とする。
3. **P3／証拠表現:** `report.md:50` の「9/9 TCP/integration tests」は厳密には **9/9 integration test functions**。通常 run で `proxy_environment_ignored_child` は環境 flag がなければ即 return (`tests/transport.rs:225-238`)、constructor test は TCP を使わない (`:271-304`)。TCP を実際に使う親 test は 7 件で、proxy child の TCP は親 test 内の subprocess で実行される。結論を変えないが、選定記録・受入証拠では区別する。

この GO は P4-15 library dependency の資格に限る。P4-16 の root Cargo/lock 追加時に exact graph の deny・transport-policy・architecture/fmt を再確認し、P4 完了は freeze §9 の scoped 実 TCP `DiscoveryService::discover` と retention/authority まで別途確認する。

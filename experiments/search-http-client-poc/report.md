# Search P4-15 HTTP client qualification — 2026-09-30

## Decision

**SELECTED for P4-16 adapter promotion:** reqwest **0.13.5**, exact Cargo pin
`=0.13.5`, `default-features = false`, features `["rustls", "gzip"]`.
The isolated PoC has its own `[workspace]` and lockfile. Root Cargo files and
production dependencies were not changed. The optional PoC feature
`ambient-proxy-canary` adds `reqwest/system-proxy` only to test defense against
future Cargo feature unification; it is not part of the selected feature set.

The fixed production interface to carry into P4-16 is:

- Trusted registration fixes the HTTPS URL at construction; caller/provider values never become a URL.
- `SystemResolver` resolves the hostname for **every call**; all A/AAAA results are checked before any connect. The checked addresses are passed to `ClientBuilder::resolve_to_addrs`, while the original hostname remains the URL/TLS identity.
- Build a fresh client per call, with `no_proxy`, `redirect(Policy::none())`, `retry(retry::never())`, no idle pool, rustls, connect/read/total bounds, and chunked body consumption. Fresh clients prevent unchecked pool reuse.
- The PoC fetch returns a bounded owned `Vec<u8>` or a low-cardinality error; dropped/cancelled requests release the response stream. P4-16 must consume those bytes inside the evaluation lease, with no raw `Vec<u8>` escape for `NO_RETENTION`. Its loopback/custom-root hook belongs behind `#[cfg(test)]`, and its production constructor remains HTTPS-only.

This selects the HTTP library and guarded pattern, **not** the production
`search-source-http` implementation or P4 end-to-end acceptance. P4-16 must
enforce registered paths/body limits, complete JSON/page/evaluation limits,
Source authority and retention leases. No real provider, credentials or
customer data were used.

## Official basis and dependency gate

- [reqwest 0.13.5 crate/API](https://docs.rs/reqwest/0.13.5/reqwest/) and [crates.io package/license](https://crates.io/crates/reqwest/0.13.5): MIT OR Apache-2.0; Rust MSRV 1.85.0 from `cargo info reqwest@0.13.5`.
- [ClientBuilder 0.13.5](https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html): `no_proxy`, redirect policy, `resolve_to_addrs`, rustls backend, connect/read timeout and pool controls. Defaults allow system proxy and redirect; [`retry::never`](https://docs.rs/reqwest/0.13.5/reqwest/retry/fn.never.html) disables the default protocol-NACK retry.
- [Response 0.13.5](https://docs.rs/reqwest/0.13.5/reqwest/struct.Response.html): `chunk` streams the body; automatic decompression can remove `Content-Length`, so the PoC checks the header when present and always counts decoded chunks.
- [IANA IPv4](https://www.iana.org/assignments/iana-ipv4-special-registry) and [IPv6](https://www.iana.org/assignments/iana-ipv6-special-registry) special-purpose registries informed the fail-closed address classifier. The static PoC ranges are mechanism evidence, not a claim that future registry updates are automatically covered.
- `cargo tree --manifest-path experiments/search-http-client-poc/Cargo.toml -e features -i reqwest` showed only `rustls` and `gzip` on the default selected graph (plus their internal features); `system-proxy` was absent. `Cargo.lock` contains 199 resolved packages including the optional canary feature and test dependencies.
- `cargo deny 0.20.2` with the local `deny.toml`: **advisories ok, bans ok, licenses ok, sources ok**. Its permissive license allowlist is MIT, Apache-2.0, BSD-3-Clause, ISC, Unicode-3.0, Zlib and CDLA-Permissive-2.0. One nonblocking `syn` 2/3 duplicate-version warning remains; no license/source/advisory exception was added.

## Real local transport evidence

Environment: macOS, Rust/Cargo 1.98.1, `cargo-deny 0.20.2`.
Builds used `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2` and `CARGO_TARGET_DIR=experiments/search-http-client-poc/target`.
All servers and the TLS certificate were created at runtime on loopback.
Response bodies existed only in memory; no fixture, response body log, cache
or retry spool was written.
After verification, `cargo clean` removed 349.7 MiB from this PoC's dedicated
target directory because shared disk space was low; source and lockfile remain.

| Case | Direct observation |
| --- | --- |
| RED before interface | Locked `cargo test --test transport` exited 101 with E0432 solely for missing `AddressResolver`, `GuardedTransport`, `Limits` and `TransportError`. |
| Redirect behavioral RED | Removing only `redirect(Policy::none())` made `redirect_never_followed` fail: it returned the second server's body instead of `Err(Redirect)`. Guard restored. |
| Proxy behavioral RED | With optional `ambient-proxy-canary`, removing only `no_proxy()` made the child test receive the local proxy response instead of the pinned direct server response. Guard restored. |
| Request bound RED/GREEN | Oversized fixed URL initially returned the wrong error (`Endpoint`); after the conservative GET framing reserve it returns `RequestLimitExceeded` before DNS/connect. |
| Default GREEN | `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=experiments/search-http-client-poc/target cargo test --manifest-path experiments/search-http-client-poc/Cargo.toml --locked` exited 0: **9/9 harness tests** (7 exercise real TCP, 1 validates constructors without a connection, and 1 child entrypoint returns without I/O in the default run), 0 unit/doc tests. |
| Unified-proxy GREEN | Same environment with `cargo test --manifest-path experiments/search-http-client-poc/Cargo.toml --locked --features ambient-proxy-canary --test transport proxy_environment_ignored -- --exact` exited 0: **1/1**. |
| Static checks | `cargo fmt --manifest-path experiments/search-http-client-poc/Cargo.toml --all` exited 0; strict `cargo clippy --manifest-path experiments/search-http-client-poc/Cargo.toml --all-targets --locked -- -D warnings` exited 0 after test-only initialization style repair. |
| Supply chain | `cargo deny --manifest-path experiments/search-http-client-poc/Cargo.toml --config experiments/search-http-client-poc/deny.toml check` exited 0 with the one duplicate-version warning above. |

The named TCP tests cover rejected redirect and ambient proxy; A/AAAA
all-address denial including metadata and IPv4-mapped IPv6, an unresolvable
hostname pinned to a validated loopback address, and a changed resolver answer
on the next call despite a reusable HTTP/1.1 connection. Runtime-generated
rustls certificate succeeds for its hostname and fails for a different
hostname on the same pinned address. An oversized `Content-Length` rejects
before a stalled body read; a small gzip wire body expanding past the decoded
limit is rejected. Synthetic TLS stall, body stall and trickle exercise the
connect/header, read and total budgets; cancellation returns `Cancelled`
and the server observes TCP EOF. An abrupt connection close produces one
observed request. The source has no filesystem/log/spool write path and
explicitly sets `retry::never()`.

## Bounds and handoff

The PoC `Limits::default` values are request 16 KiB, decoded response
1 MiB, connect/header 500 ms, each body read 500 ms and total call 2 s.
Tests lower selected limits (80/90/180 ms and 32/128 bytes) to reach
boundaries quickly. The connect bound deliberately includes response
headers because reqwest does not expose that phase separately. The call-wide
2 s bound includes DNS, connect and body; the P4 5 s evaluation budget belongs
to the application owner and was not implemented here.

The `no_automatic_retry_or_disk_spool` canary measures one TCP attempt after
an abrupt close. Protocol-NACK retry suppression is supported by the explicit
`retry::never()` API and official documentation, rather than a simulated
HTTP/2 NACK. The no-spool conclusion is limited to this PoC's source path
(no `fs`/logging calls) and streaming API usage. No claim is made about
future adapter code or deployed runtime.

**PoC code bundle SHA-256:** `1f42e2a0d54d2dda68cdac5c7d2e1aa29539309c30dbd72b44fabaa66222bd50`.
This hashes, in order, `Cargo.toml`, `Cargo.lock`, `deny.toml`,
`src/lib.rs`, `tests/transport.rs`; each entry is framed by a 4-byte
big-endian relative-path length, path bytes, 8-byte big-endian content
length and content bytes. It excludes this report to avoid self-reference.
No Git commit was made by P4-15.

## Parent evidence wording correction — 2026-09-30

Independent source inspection counted 7 real-TCP test functions, one constructor-only test, and one child entrypoint that is idle unless spawned by the proxy canary. The default test summary remains 9/9 passed; it does not mean 9 distinct TCP tests. The separate proxy feature canary executes its child TCP path. Code bundle and measured observations are unchanged. Library promotion remains subject to the independent review and P4-16 production boundaries.

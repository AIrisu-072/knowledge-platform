# Search Platform Completion — local environment inventory

観測日時: 2026-09-30 14:31 JST。対象 checkout: `feat/search-platform-completion-program` / `2acbca2`。これは read-only inventory であり、build、test、download、Docker container 起動、service 起動は実施していない。後続の資格試験結果ではない。

## 観測した実行環境と容量

| 対象 | 観測結果 | 境界 |
| --- | --- | --- |
| Host | macOS `Darwin arm64`、10 CPU、物理 RAM 16 GiB、`memory_pressure -Q` の system-wide free 51% | 実行時の残量は再計測が必要。 |
| Disk | checkout と `/tmp` の APFS は 460 GiB 中 420 GiB 使用、空き 12 GiB、使用率 98%。この worktree の `target/` は 118 MiB | `CARGO_TARGET_DIR` は unset。Phase D の reduced-debug `mise run verify` は link 中に `errno=28` で停止し、local standard gate は GREEN ではない（`docs/superpowers/execution/search-discovery-platform-v0-acceptance.md`）。同時 build で空きが急減し得る。 |
| Rust | `rustc`/`cargo` 1.98.1、`clippy` 0.1.98、`rustfmt` 1.9.0。`rust-toolchain.toml` も 1.98.1 | 実行体は `~/.cargo/bin/`。この inventory では compile していない。 |
| mise / JS | `mise` 2026.8.4。`mise.toml` は Node 24.21.0、pnpm 12.4.1、cargo-nextest 0.9.144 を pin。`mise exec -- node --version` と `mise exec -- pnpm --version` はそれぞれ pin と一致 | 直呼び `node` は 26.3.1、直呼び `pnpm --version` は ENOEXEC。JS gate は `mise exec` / `mise run` の解決経路を使う。 |
| nextest | pin された実体 `~/.local/share/mise/installs/cargo-cargo-nextest/0.9.144/bin/cargo-nextest` は 0.9.144 | 現 shell と `mise exec -- sh -c 'cargo nextest --version'` は `~/.cargo/bin/cargo-nextest` の 0.9.143 を解決する。repository の `mise run test:rust` は `cargo nextest run --workspace` を呼ぶため、厳密な pin が必要な実行前に subcommand 解決を再確認する。 |
| Docker | client/server 29.4.0、Linux daemon 応答あり。ローカル `postgres:18.6-bookworm` は `sha256:3725f4e2499e…`、Linux arm64、表示サイズ 647 MB | image pull、container 起動はしていない。Testcontainers の起動・migrations 成功は今回未検証。 |

## PDFium pin と利用可能な実体

- `experiments/document-semantic-inspection/scripts/install-pdfium.sh` は `chromium/7881` / `151.0.7881.0`、mac-arm64 の `libpdfium.dylib` SHA-256 `1bc45b15466b34cef96641ce25c77a876e70010c6b114f909dda2f5325fc5bd7` を固定し、`experiments/document-semantic-inspection/target/dsi-poc/pdfium/7881/mac-arm64/lib/` を返す。`crates/document-semantic-inspection-worker/src/adapters/pdf.rs::pdfium` は `PDFIUM_DYNAMIC_LIB_PATH` のライブラリを実行時に同じ SHA で検証する。CI は installer の返す path を環境変数に設定する（`.github/workflows/ci.yml`）。
- この checkout の固定 install path は**不存在**、`PDFIUM_DYNAMIC_LIB_PATH` は**unset**。installer をここで実行すると download の可能性があるため実行していない。
- 次の既存 `lib/` directory では、各 `VERSION` が `151.0.7881.0`、各 dylib の実測 SHA-256 が上記 pin と一致した。先頭の path をこの host の bounded PDF canary に使える。いずれも別 worktree の一時的な path なので、使用直前に存在と hash を再確認する。
  - `$HOME/.codex/worktrees/document-management-basics-v0-d/knowledge-platform/experiments/document-semantic-inspection/target/dsi-poc/pdfium/7881/mac-arm64/lib/`
  - `$HOME/.codex/worktrees/document-publication-end-v0-implementation/knowledge-platform/experiments/document-semantic-inspection/target/dsi-poc/pdfium/7881/mac-arm64/lib/`
  - `$HOME/.codex/worktrees/document-versioning-v0-implementation/knowledge-platform/experiments/document-semantic-inspection/target/dsi-poc/pdfium/7881/mac-arm64/lib/`
  - `$HOME/.codex/worktrees/dsi-v0-production-task4/knowledge-platform/experiments/document-semantic-inspection/target/dsi-poc/pdfium/7881/mac-arm64/lib/`
- この inventory は dylib の dynamic bind、PDF test、Linux x64 ライブラリの現地実体を検証していない。PoC qualification plan の `Task 6` と現行 CI は pinned PDFium を使う。Search Extraction の parser 忠実度は別資格試験である。

## 実 PostgreSQL fixture と build の入口

- 既存の read-only source 確認では `crates/search-source-document/tests/postgres_snapshot.rs::Fixture::new` が `GenericImage::new("postgres", "18.6-bookworm")` を Testcontainers `AsyncRunner` で起動し、mapped port の `PgPool` に `document_repository_postgres::migrate` を実行する。`tests/vertical_slice.rs` は `../../document-repository-postgres/tests/support/versioning.rs::Fixture` を使用する。`tests/outbox_indexing.rs::failed_indexing_leaves_committed_document_and_generic_outbox_untouched` も同じ PG image を直接使う。image は上記のとおり既存なので、明示 pull は不要。
- PG 最小 canary 候補は `cargo test --locked -p search-source-document --test postgres_snapshot live_and_historical_enumeration_use_t10_operation_not_current_null_inference -- --exact --test-threads=1`。本文 coverage 境界は `--test vertical_slice body_required_coverage_returns_gap_before_real_title_lexical_port_is_called` の focused case で確認できる。**コマンドは提案であり、今回未実行**。
- 既存 worker build の正本は `mise.toml` の `test:rust`。Linux では `cargo build --locked -p document-semantic-inspection-worker --bin document-semantic-inspection-worker` の後、`target/debug/document-semantic-inspection-worker` を `DSI_WORKER_BIN` に設定して `cargo nextest run --workspace` を呼ぶ。現 macOS checkout に worker binary は**不存在**。P1 の `search-extraction-worker` は `p1-extraction-design.md` の提案であり、package/binary は**未実装**。

## 合成 smoke seed と bounded 実行順

既存の versioned DSI seed は `experiments/document-semantic-inspection/fixtures/manifest.json` に DSI 用 SHA/期待値がある。以下は P1 corpus の**入力候補**であり、Search Extraction の Unit text/native locator/coverage 合格を示さない。

| 形式 | 既存 seed path | 境界 case |
| --- | --- | --- |
| TXT / CSV / HTML | `fixtures/txt/base.txt`、`fixtures/csv/base.csv`、`fixtures/html/base.html` | `txt/unicode-nfd.txt`、`csv/quote-noise.csv`、`html/js-only.html` |
| DOCX / XLSX / PPTX | `fixtures/docx/base.docx`、`fixtures/xlsx/base.xlsx`、`fixtures/pptx/base.pptx` | `docx/table-merge-change.docx`、`xlsx/formula-source-change-same-cache.xlsx`、`pptx/table-change.pptx` |
| PDF | `fixtures/pdf/base.pdf` | `pdf/scan-only.pdf`、`pdf/ambiguous-read-order.pdf` |

表の path はすべて `experiments/document-semantic-inspection/` を起点とする。`experiments/search-extraction-poc/` と `crates/search-extraction-*` は現 checkout に**不存在**。P1 設計が要求する日本語・ZIP 入れ子・同文言の複数 ContentItem・独立 locator oracle などはこの seed 一覧だけでは**未検証**。

後続の build/test は、空き容量を再計測し、`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2` を付けた focused crate/test から始める。これらは過去の Phase A 容量診断で使われた設定で、成功保証ではない。PDF canary では上記の検証済み `lib/` を `PDFIUM_DYNAMIC_LIB_PATH` に指定する。フル `mise run verify` は局所 canary と容量を確認してから一度だけ実行する。`Cargo.toml`、`Cargo.lock`、`mise.toml` の共有変更は parent が直列化する（`p1-extraction-design.md` P1-3）。

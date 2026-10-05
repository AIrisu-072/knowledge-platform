意味保存の日本語訳。承認・資格の追加ではない。記載の既存ハッシュは原文/原証拠を指す。

[固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/environment.md)

<a id="search-platform-completion--local-environment-inventory"></a>
# Search Platform Completion — ローカル環境の調査記録

> 以下の状態・次の作業は当時の記録であり、現在の実行指示ではありません。現行の停止条件・実行可否は[最新の実行状態](../../execution/search-platform-completion-program-status.md)を参照してください。

観測日時は2026-09-30 14:31 JSTです。対象checkoutは`feat/search-platform-completion-program` / `2acbca2`です。これは読み取り専用の環境調査であり、ビルド、テスト、ダウンロード、Dockerコンテナの起動、サービスの起動は実施していません。後続の資格試験の結果ではありません。

## 観測した実行環境と容量

| 対象 | 観測結果 | 適用範囲と制約 |
| --- | --- | --- |
| ホスト | macOS `Darwin arm64`、10 CPU、物理RAM 16 GiB。`memory_pressure -Q`によるシステム全体の空きは51% | 実行時に残量の再計測が必要です。 |
| ディスク | checkoutと`/tmp`のAPFSは460 GiB中420 GiB使用、空き12 GiB、使用率98%。この作業ツリーの`target/`は118 MiB | `CARGO_TARGET_DIR`は未設定です。Phase Dのデバッグ情報を減らした`mise run verify`は、リンク中に`errno=28`で停止し、ローカル標準ゲートはGREENではありません（`docs/superpowers/execution/search-discovery-platform-v0-acceptance.md`）。同時ビルドで空きが急減する可能性があります。 |
| Rust | `rustc`/`cargo` 1.98.1、`clippy` 0.1.98、`rustfmt` 1.9.0。`rust-toolchain.toml`も1.98.1 | 実行ファイルは`~/.cargo/bin/`にあります。この調査ではコンパイルしていません。 |
| mise / JS | `mise` 2026.8.4。`mise.toml`はNode 24.21.0、pnpm 12.4.1、cargo-nextest 0.9.144を固定しています。`mise exec -- node --version`と`mise exec -- pnpm --version`は、それぞれ固定値と一致します | 直接呼び出した`node`は26.3.1、直接の`pnpm --version`はENOEXECです。JSゲートは`mise exec` / `mise run`の解決経路を使います。 |
| nextest | 固定された実体`~/.local/share/mise/installs/cargo-cargo-nextest/0.9.144/bin/cargo-nextest`は0.9.144 | 現在のシェルと`mise exec -- sh -c 'cargo nextest --version'`は、`~/.cargo/bin/cargo-nextest`の0.9.143を参照します。リポジトリの`mise run test:rust`は`cargo nextest run --workspace`を呼ぶため、厳密なバージョン固定が必要な実行の前に、サブコマンドの解決先を再確認します。 |
| Docker | クライアント/サーバーは29.4.0、Linuxデーモンの応答があります。ローカルの`postgres:18.6-bookworm`は`sha256:3725f4e2499e…`、Linux arm64、表示サイズ647 MB | イメージの取得やコンテナ起動はしていません。Testcontainersの起動・マイグレーションの成功は今回未検証です。 |

<a id="pdfium-pin-と利用可能な実体"></a>
## PDFiumの固定バージョンと利用可能な実体

- `experiments/document-semantic-inspection/scripts/install-pdfium.sh`は、`chromium/7881` / `151.0.7881.0`、mac-arm64の`libpdfium.dylib`のSHA-256 `1bc45b15466b34cef96641ce25c77a876e70010c6b114f909dda2f5325fc5bd7`を固定し、`experiments/document-semantic-inspection/target/dsi-poc/pdfium/7881/mac-arm64/lib/`を返します。`crates/document-semantic-inspection-worker/src/adapters/pdf.rs::pdfium`は、`PDFIUM_DYNAMIC_LIB_PATH`のライブラリを実行時に同じSHAで検証します。CIは、インストーラーが返すパスを環境変数に設定します（`.github/workflows/ci.yml`）
- このcheckoutでは固定インストール先が**不存在**で、`PDFIUM_DYNAMIC_LIB_PATH`は**未設定**です。ここでインストーラーを実行するとダウンロードの可能性があるため、実行していません
- 次の既存の`lib/`ディレクトリでは、各`VERSION`が`151.0.7881.0`で、各dylibの実測SHA-256が上記の固定値と一致しました。先頭のパスは、このホストで範囲を限定したPDFの小規模確認に使えます。いずれも別作業ツリーの一時的なパスなので、使用直前に存在とハッシュを再確認します
  - `$HOME/.codex/worktrees/document-management-basics-v0-d/knowledge-platform/experiments/document-semantic-inspection/target/dsi-poc/pdfium/7881/mac-arm64/lib/`
  - `$HOME/.codex/worktrees/document-publication-end-v0-implementation/knowledge-platform/experiments/document-semantic-inspection/target/dsi-poc/pdfium/7881/mac-arm64/lib/`
  - `$HOME/.codex/worktrees/document-versioning-v0-implementation/knowledge-platform/experiments/document-semantic-inspection/target/dsi-poc/pdfium/7881/mac-arm64/lib/`
  - `$HOME/.codex/worktrees/dsi-v0-production-task4/knowledge-platform/experiments/document-semantic-inspection/target/dsi-poc/pdfium/7881/mac-arm64/lib/`
- この調査では、dylibの動的結合、PDFテスト、Linux x64ライブラリの現地実体を検証していません。PoC資格試験計画の`Task 6`と現行CIは、固定済みPDFiumを使います。Search Extractionのパーサー忠実度は、別の資格試験の対象です

<a id="実-postgresql-fixture-と-build-の入口"></a>
## 実PostgreSQLフィクスチャとビルドの入口

- 既存ソースの読み取り専用確認では、`crates/search-source-document/tests/postgres_snapshot.rs::Fixture::new`は、`GenericImage::new("postgres", "18.6-bookworm")`をTestcontainersの`AsyncRunner`で起動し、割り当てられたポートの`PgPool`に対して`document_repository_postgres::migrate`を実行します。`tests/vertical_slice.rs`は`../../document-repository-postgres/tests/support/versioning.rs::Fixture`を使用します。`tests/outbox_indexing.rs::failed_indexing_leaves_committed_document_and_generic_outbox_untouched`も、同じPGイメージを直接使います。イメージは上記のとおり存在するため、明示的な取得は不要です
- PGの最小確認候補は`cargo test --locked -p search-source-document --test postgres_snapshot live_and_historical_enumeration_use_t10_operation_not_current_null_inference -- --exact --test-threads=1`です。本文の網羅性の境界は、`--test vertical_slice body_required_coverage_returns_gap_before_real_title_lexical_port_is_called`の対象限定ケースで確認できます。**コマンドは提案であり、今回未実行**です
- 既存ワーカービルドの正本は、`mise.toml`の`test:rust`です。Linuxでは、`cargo build --locked -p document-semantic-inspection-worker --bin document-semantic-inspection-worker`の後、`target/debug/document-semantic-inspection-worker`を`DSI_WORKER_BIN`に設定して`cargo nextest run --workspace`を呼びます。現在のmacOS checkoutにワーカーバイナリは**不存在**です。P1の`search-extraction-worker`は`p1-extraction-design.md`の提案であり、パッケージとバイナリは**未実装**です

<a id="合成-smoke-seed-と-bounded-実行順"></a>
## 合成スモークテスト用の元データと対象限定の実行順

既存のバージョン管理済みDSI元データは、`experiments/document-semantic-inspection/fixtures/manifest.json`にDSI用のSHAと期待値があります。以下はP1コーパスの**入力候補**であり、Search ExtractionのUnitテキスト・ネイティブ位置情報・網羅性の合格を示しません。

| 形式 | 既存の元データのパス | 境界ケース |
| --- | --- | --- |
| TXT / CSV / HTML | `fixtures/txt/base.txt`、`fixtures/csv/base.csv`、`fixtures/html/base.html` | `txt/unicode-nfd.txt`、`csv/quote-noise.csv`、`html/js-only.html` |
| DOCX / XLSX / PPTX | `fixtures/docx/base.docx`、`fixtures/xlsx/base.xlsx`、`fixtures/pptx/base.pptx` | `docx/table-merge-change.docx`、`xlsx/formula-source-change-same-cache.xlsx`、`pptx/table-change.pptx` |
| PDF | `fixtures/pdf/base.pdf` | `pdf/scan-only.pdf`、`pdf/ambiguous-read-order.pdf` |

表のパスはすべて`experiments/document-semantic-inspection/`を起点とします。`experiments/search-extraction-poc/`と`crates/search-extraction-*`は、現在のcheckoutに**不存在**です。P1設計が要求する日本語、ZIP入れ子、同文言の複数ContentItem、独立した位置情報の正解判定などは、この元データ一覧だけでは**未検証**です。

後続のビルド・テストは、空き容量を再計測し、`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2`を付けて、対象を限定したcrate・テストから始めます。これらは過去のPhase A容量診断で使われた設定であり、成功を保証しません。PDFの小規模確認では、上記の検証済み`lib/`を`PDFIUM_DYNAMIC_LIB_PATH`に指定します。フルの`mise run verify`は、局所的な確認と容量確認を終えてから一度だけ実行します。`Cargo.toml`、`Cargo.lock`、`mise.toml`の共有変更は親担当が直列化します（`p1-extraction-design.md` P1-3）。

# デスクトップ原本プロキシの単体検証

Tauri の UI スタックを解決せず、`apps/desktop/src-tauri/src/proxy.rs` の実ソースを `#[path]` で直接コンパイルする補助 workspace です。`tauri::http` と `async_runtime::block_on` の薄いテスト用置換だけを提供します。依存の直接バージョンは desktop の Cargo.lock と同じ固定値です。

```sh
CARGO_INCREMENTAL=0 cargo test --manifest-path tools/desktop-proxy-qualification/Cargo.toml --locked
cargo fmt --manifest-path tools/desktop-proxy-qualification/Cargo.toml --check
```

実ソースの通常上限・要求ヘッダーの転送制限・応答 MIME 無害化・新しい viewer 用の下限指定を検証します。viewer 指定は GET の正整数1件のみ受理し、既存上限を引き上げません。誤形式は backend 接続前に拒否し、ヘッダー自体は backend に転送しません。Content-Length と実受信 chunk の双方に制限を適用します。

これはフル Tauri ビルド、WebView の動作、実機パッケージ、OS によるメモリ上限の検証ではありません。通常ダウンロードの上限は維持します。viewer 10 MiB は論理応答本文の上限で、ネットワーク chunk の一時割当や Vec capacity、PDF parser 全体の heap の厳密上限は保証しません。

既存の `mise run desktop:bridge:e2e` はこの harness 試験を最初に実行し、続けて既存の Workspace broker とブラウザー試験を実行します。通常 CI の必須 `desktop-runtime-bridge` ジョブもこの同じタスクを使用します。

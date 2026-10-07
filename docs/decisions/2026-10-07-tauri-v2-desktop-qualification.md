# Tauri v2 Desktopの資格確認結果と依頼者判断

日付：2026-10-07 UTC。状態：**依頼者が5項目すべてに合意（2026-10-07）。Tauri shellを `apps/desktop/src-tauri` に追加し、Linuxで実GUIの確認まで実施済み。Windows実機は未実施。**

## 依頼者の判断（2026-10-07、全項目合意）

依頼者の発言：「判断するところは全て合意します。クラウドセッションで行っているのでTauriをそちらで構築してPlaywrightとかのデスクトップアプリのGUIからの確認とかもして欲しい。CIにして欲しいわけではないです。」

| # | 判断事項 | 合意内容と、今回の適用 |
|---|---|---|
| 1 | MPL-2.0の例外 | desktop用lock（`apps/desktop/src-tauri/Cargo.lock`）に限り、版を固定して許可。`option-ext 0.2.0`（実行ファイルに入る）、`cssparser 0.37.0`・`cssparser-macros 0.7.1`・`selectors 0.38.0`・`dtoa-short 0.3.5`（build時だけ）。[desktop用deny.toml](../../apps/desktop/src-tauri/deny.toml)。注意書きは[THIRD_PARTY_NOTICES](../../apps/desktop/THIRD_PARTY_NOTICES.md) |
| 2 | Linux専用advisoryの扱い | Linuxでの利用を承認。desktop lockに限り、OSVで `RUSTSEC-2024-0429`（glib 0.18.5、別名GHSA-wrw7-89jp-8q8g）と `RUSTSEC-2024-0370`（proc-macro-error 1.0.4）を無視。期限は2027-04-07で、恒久ignoreにはしない（[osv-scanner.toml](../../apps/desktop/src-tauri/osv-scanner.toml)）。cargo-denyは `RUSTSEC-2024-0370` だけを検出したため、それだけを無視 |
| 3 | Windowsでの検証経路 | Windows CIは追加しない（`allow_native_windows = false` のまま）。依頼者のWindows実機で手動確認する。手順は[操作手順](../operations/desktop-workspace-runtime.md) |
| 4 | WebView2 Runtime | 同梱・bootstrapper配布はしない。端末にインストール済みのEvergreen Runtimeを使う |
| 5 | OSV送信 | desktop lockの公開依存名と版を、既存CIの `osv-scanner scan source -r .` がapi.osv.devへ送ることを承認 |

PR52の限定例外（2026-10-10までの非build一覧検証）は使っていません。上記はそれとは別の、今回の承認に基づく例外です。

## 確認した事実

### 依存とlicense（実lock、2026-10-07）

- `tauri = "=2.12.1"`（default featureを使わず `wry`・`x11`・`common-controls-v6`・`custom-protocol`）、`tauri-build = "=2.7.1"`、`rfd = "=0.16.0"`（`gtk3`、xdg-portal無し）、`reqwest = "=0.13.5"`（TLS・proxy・HTTP/2無し）。lockは420 package。
- desktop crateはroot workspaceの外（独立したCargo workspace）。root CIの `cargo check/clippy/test --workspace` と root `cargo deny check` はTauriを解決しません。
- `cargo deny check`（desktopディレクトリ）：advisories ok、bans ok、licenses ok、sources ok。
- MPL-2.0の5件のうち、実行ファイルに入るのは `option-ext` だけでした（Linux・Windowsとも `dirs` 経由）。他の4件は `tauri-macros`／`tauri-codegen` のbuild時だけです。
- **新たに判明**：`target-lexicon 0.12.16` が `Apache-2.0 WITH LLVM-exception` です。Linuxのbuild時だけ（`system-deps` → `cfg-expr`）で、実行ファイルには入りません。LLVM例外はApache-2.0（許可済み）に許諾を足すだけなので、版を固定して許可しました。承認一覧に無かった項目として、ここに明記します。
- Windows向けの通常依存（`cargo tree --target x86_64-pc-windows-msvc -e normal`）に、glib・gtk・proc-macro-errorは含まれません。

### OSV

- この環境からapi.osv.devへは接続できません（通信方針）。PR53のCI（security job）のログで、Tauri lockに対してOSVが検出したのは上記2件だけであることを確認しました。
- 設定ファイルはlockと同じディレクトリに置きます。既存の `experiments/search-vector-model-poc/osv-scanner.toml` も同じ置き方で、そのディレクトリのlockにだけ適用されています（ローカル実行時の「unused ignores」表示で、ディレクトリ単位で読み込まれることを確認）。
- 最終的な合否は、PRの既存security jobで確認します。

### 実装と実行（Linux）

- Ubuntu 24.04、WebKitGTK 2.52.6、GTK 3.24.41。debug buildで起動・操作できました。
- **WebKitGTKの不具合を発見**：アプリ自身のURL scheme（`tauri://`）への要求の本文がBlob／FormDataだと、`webkit_uri_scheme_request_get_http_body()` の中でSIGSEGVし、アプリごと落ちます（gdbでstackを確認）。文字列・バイト列の本文は2MBでも正常です。文書の登録（multipart）は必ず該当するため、shellの初期化scriptで、同一originへ送る本文をページ内でArrayBufferに確定してから送るようにしました。送られるbytes・method・header（multipartのboundaryを含む）は変わりません。
- WebKitGTKは、同一originのcustom scheme要求にOriginヘッダーを付けません（Refererのみ）。このためOrigin必須の検査はできず、「付いていれば自アプリと一致すること」を求める多層防御にしました。主たる境界は「scheme自体をmain windowにだけ提供し、main windowは自アプリのURLにしか遷移できない」ことです。

## shellの構成（実装済み）

- 単一main window（コードで生成）。新しいウィンドウは拒否し、遷移は自アプリのURLと、自アプリが作ったblob URL（「原本を取得」のダウンロード用）だけを許可します。
- IPC commandは `local_workspace_runtime` だけを登録します。capabilityは `main` windowのローカル（同梱）originだけに、このcommandの許可1件を与えます。Tauriのcoreやplugin（fs・shell・dialog・http等）の権限は与えません。
- フォルダー選択は `rfd` の単一フォルダー選択だけです。選んだ絶対pathはbrokerへ直接渡し、画面には不透明なIDだけを返します。
- `/v1` は、shellのURL schemeから、環境変数 `KNOWLEDGE_PLATFORM_API_ORIGIN` で指定した1つのloopback（`http://127.0.0.1:<port>` または `http://[::1]:<port>`）backendへ転送します。要求・応答のheaderは許可リスト方式、redirectは追わず、cookieとsystem proxyは使わず、大きさと時間に上限があります。既存serverのCORSは変えていません。
- CSPは既存previewと同等に、Tauri IPC用の `ipc:` と `http://ipc.localhost` を `connect-src` に加えたものです。
- ダウンロードは、自アプリのblob URLを、利用者のDownloadsフォルダー直下へ保存する場合だけ許可します。
- 状態フォルダーは、OSのアプリデータフォルダー（Linuxでは `~/.local/share/dev.knowledgeplatform.desktop/workspace-runtime`）です。

## 証拠として扱わないもの

- LinuxでのbuildやGUI確認は、Windows・WebView2での動作の証拠ではありません。Windows版のbrokerは引き続きfail-closed（すべて「利用できません」）です。
- desktop crateは `x86_64-pc-windows-gnu` 向けの `cargo clippy --all-targets -- -D warnings` が通ります（MinGWのresource compilerを使ったcompile確認だけ）。MSVC版のbuild、実行、WebView2での動作は確認していません。compileできることは、Windowsでの動作の証拠ではありません。
- テスト専用stdio bridgeを使ったChromiumの通しE2Eは、Tauriの証拠ではありません。

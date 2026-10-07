# デスクトップ版とローカルWorkspace Runtime：操作と検証の手順

対象：同じReact画面をブラウザーとデスクトップで動かすためのRuntime Contract、端末ローカルのWorkspace broker（`crates/local-workspace-runtime`）、Tauri v2のdesktop shell（`apps/desktop/src-tauri`）。
本番サーバーへの反映、認証・本番データ・外部サービスへの接続は、この手順には含みません。

## いま使えること

| 実行環境 | できること |
|---|---|
| ブラウザー版（既存のpreview・本番Web） | これまでどおり文書・タスク・検索が使えます。`/local-workspaces` を開くと「ブラウザー版ではローカルフォルダーとWorkspaceを利用できません」と表示され、作成・追加などのボタンは出ません |
| デスクトップ版（Linux） | **起動して使えます。** 既存の文書・タスク・検索の画面がそのまま動き、APIはshell経由で手元のserverへ届きます。ローカルWorkspaceの作成・名前変更・フォルダーの追加と解除・閲覧・ファイル作成・再起動後の復元が使えます。2026-10-07にクラウド環境の実GUIで確認しました（下記） |
| デスクトップ版（Windows） | build手順はあります。**Windows実機では未確認です。** ローカルWorkspaceの機能は、Windows用のbroker実装と実機検証が済むまで、すべて「この実行環境ではローカルフォルダーを利用できません。」と表示されます（fail-closed） |

デスクトップ版のローカルWorkspaceでできること：

- 「新しいWorkspace」で名前を入れて作成します。端末内の管理フォルダーが自動で作られます。フォルダーの場所はWorkspace名と無関係で、名前を変えても場所は変わりません。
- 「フォルダーを追加」でOSのフォルダー選択画面を開きます。取り消した場合は何も変わりません。選んだフォルダーは参照として登録され、中身は移動もコピーもされません。
- 「開く」でフォルダー内を一覧（100件ずつ）します。もう一度「開く」を押すと一覧を取り直します。下の階層への移動と「上の階層へ」、テキストの内容表示（先頭1MiBまで）ができます。
- 「この場所にファイルを作成」で新しいファイルを保存します（8MiBまで、同名のファイルは上書きしません）。
- 「解除」でフォルダーの登録だけを外します。中身は削除されません。管理フォルダーは解除できません。
- アプリを再起動すると、Workspace・名前・フォルダーの登録が復元されます。開いていた読み取りは復元されません。

安全のため開けないもの：シンボリックリンク・ジャンクション、2つ以上の場所からリンクされたファイル、通常のファイルでないもの、アプリの管理領域、`..` や絶対pathのような指定、Windowsの予約名（CON等）。

デスクトップ版のshellがしないこと：

- ウィンドウは1つだけです。新しいウィンドウは開きません。アプリ外のURL（Webサイト等）へは移動しません。
- 画面から使えるOSの機能は、上記のローカルWorkspace操作だけです。shell・ファイル操作・外部プログラム起動などのpluginは入っていません。
- ダウンロード（「ファイルを取得」）は、Downloadsフォルダーの直下にだけ保存します。同名のファイルがあれば番号を付けて別名にします。

## デスクトップ版のbuildと起動（Linux）

前提（Ubuntu 24.04で確認）：

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
  libjavascriptcoregtk-4.1-dev librsvg2-dev libxdo-dev build-essential pkg-config
```

build（本番の画面をbuildしてから、それを埋め込んだshellをbuildします）：

```bash
mise run desktop:build
# リリース版：pnpm --filter @knowledge-platform/document-web build の後に
# cargo build --release --locked --manifest-path apps/desktop/src-tauri/Cargo.toml
```

起動（接続先のserverを、環境変数で1つだけ指定します）：

```bash
KNOWLEDGE_PLATFORM_API_ORIGIN=http://127.0.0.1:8080 \
  apps/desktop/src-tauri/target/debug/knowledge-platform-desktop
```

接続先の決まり：

- 指定できるのは、手元のPCのloopbackのserverだけです（`http://127.0.0.1:<port>` または `http://[::1]:<port>`）。portの指定が必要です。`localhost`・他のPC・`https://` は受け付けません（認証の設計がまだ無いため）。
- 画面は今までどおり `/v1/...` を呼び、shellがそれを指定のserverへ転送します。転送するのは `/v1` 以下だけです。cookie・認証情報・利用者を名乗るheaderは送りません。
- 未設定または形式が違う場合、文書・タスクの画面は「読み込みに失敗しました」を表示します（ローカルWorkspaceは使えます）。
- serverには、既存のPoC実行手順（`tools/document-poc-runtime`、`tools/organization-poc-runtime`）で起動する合成データのserverを使ってください。

## 開発者向けの検証コマンド

```bash
# broker（Linux）
cargo test -p local-workspace-runtime
cargo clippy -p local-workspace-runtime --all-targets --locked -- -D warnings

# frontend：全GUI試験・型・build
pnpm --filter @knowledge-platform/document-web test
pnpm --filter @knowledge-platform/document-web typecheck
pnpm --filter @knowledge-platform/document-web build

# Chromium＋同じReact build＋Desktop adapter＋実broker（テスト専用stdio bridge。CIのdesktop-runtime-bridge job）
mise run desktop:bridge:e2e

# desktop shell（独立したCargo workspace）：fmt・clippy・単体試験・desktop用の依存policy
mise run desktop:check

# 実アプリのGUI通し確認（ローカル専用。CIでは実行しません）
mise run desktop:gui:e2e
```

`desktop:gui:e2e` は、本物のデスクトップアプリ（Tauri＋WebKitGTK＋埋め込んだ本番画面＋broker）を、その画面から操作して確かめます。

- 使うもの：tauri-driver 2.1.0（`cargo install tauri-driver --version =2.1.0 --locked`。miseの `[tools]` には入れていません。追跡対象の `mise.lock` が変わり、CIの作業ツリー検査が失敗するため）、WebKitWebDriver（`webkit2gtk-driver`）、Xvfb、xdotool（OSのフォルダー選択画面を操作）、ImageMagick、Docker（PostgreSQL 18.6）、PDFium（`KP_DSI_PDFIUM_RUNTIME_DIR`、`experiments/document-semantic-inspection/scripts/install-pdfium.sh`）。
- 毎回、使い捨てのPostgreSQLとorganization-server（合成の `sales-01`）を起動し、合成文書を登録してから確認します。HOME・設定・データは実行ごとの一時フォルダーに分けます。終了時は、自分が作ったcontainerとprocessだけを止めます。
- 結果は `apps/desktop/e2e/.state/run-*/report.json` とスクリーンショットに残ります（git管理外）。
- これはLinuxでの証拠です。Windows・WebView2の証拠にはなりません。

2026-10-07の確認結果（16シナリオ、99項目、すべて成功）：

| 確認したこと | 主な確認内容 |
|---|---|
| 起動と既存画面 | ウィンドウ1つ、文書一覧に実APIの合成文書、詳細への移動と「戻る」 |
| Router・Query | `/tasks` を直接開く（SPAのfallback）、タスク・検索・文書・編集作業への移動、タスク一覧にWork APIの内容 |
| キーボードとfocus | 最初のTabで「メインコンテンツへ」、focus表示、Enterでmainへ、ダイアログの開閉とfocusの戻り |
| reduced motion | 既定では動きあり。GTKの「アニメーション無効」設定で `prefers-reduced-motion` が成立し、motion tokenが0msになる |
| API転送 | 約2.8MBのmultipart上りと下りのbyte列が一致、名乗りheaderは無視、`/v1/../` や `%2e%2e` で外へ出ない、OPTIONSは405、backendへの直接接続はCSPで遮断、「ファイルを取得」でDownloadsへ保存 |
| 画面からの文書登録 | ファイルを選んで「下書きとして登録」→詳細画面、APIから取り出した原本が一致 |
| ローカルWorkspace | 作成・名前変更、管理フォルダーへの作成と表示、同名・不正な名前（`../`、`CON`、`a/b`）の拒否 |
| フォルダー選択 | 実際のGTKの選択画面で、取消・選択・選択中の2つ目の要求（picker_busy）・同じフォルダーの再追加の拒否 |
| 閲覧と制限 | 一覧、階層移動、1MiBを超えるファイルは先頭だけ、symlinkは一覧に出さない、hardlinkは開かない |
| 不正な要求 | 画面のscriptから `..`・絶対path・偽ID・未知のcommand・余分なfield・plugin（fs・shell・window）を送っても拒否。新しいウィンドウと外部URLへの移動も拒否 |
| 置き換え・脱出 | 一覧の後でsymlinkに差し替えた階層は開かない、フォルダー自体の置き換えを検出して古い一覧は出さない、解除しても中身は残る |
| 再起動と再送 | 再起動後もWorkspace・名前・追加フォルダー・管理フォルダーの内容を復元、同じ操作IDの再送は同じ結果（ファイルは1つ）、内容が違えば拒否、同時の同一操作は1つに収束 |
| 二重起動 | 2つ目のアプリは「デスクトップ版が別に起動しています。」と表示し、1つ目は使い続けられる |
| backend停止・未設定 | 文書画面は失敗を表示し、ローカルWorkspaceは使える。未設定のときは503 |

## 状態の保存場所と復旧

brokerは、OSのアプリデータフォルダー（Linuxでは `~/.local/share/dev.knowledgeplatform.desktop/workspace-runtime`）に次のものを保存します。

- `registry.json`：Workspace・登録・操作IDの記録。権限0600で、一時ファイル→fsync→renameの順に原子的に置き換えます。
- `managed/<ランダムID>`：管理フォルダー（権限0700）。
- `.lock`：同時起動の防止。

| 表示 | 原因と対処 |
|---|---|
| 「デスクトップ版が別に起動しています」 | 同じ状態フォルダーを別のプロセスが使っています。もう一方を終了してから開き直してください |
| 「ローカルWorkspaceの記録を読み取れません」 | `registry.json` が壊れているか、読み取れません。記録を守るため自動では初期化しません。アプリを終了し、状態フォルダーごとバックアップを取ってから担当者へ連絡してください。管理フォルダーの中身はそのまま残っています |
| 「フォルダーが移動・削除・置き換えされたため利用できません」 | 登録したフォルダーの実体が変わりました。同じ名前で作り直したフォルダーも別物として扱います。「解除」してから選び直してください |
| 「結果を確認できませんでした」 | 応答が届きませんでした。「結果を確認」を押すと、**同じ操作ID**で結果を確認します。管理フォルダーやファイルが二重に作られることはありません。確認するまでは他のWorkspaceやフォルダーへ移動できず、画面を離れて戻っても同じ場所に「結果を確認」が残ります（アプリを終了すると画面側の保持は消えますが、broker側の記録は残ります） |
| 「安全に読み取れる状態を確認できない」 | Linuxで、自分が所有していないファイル、network/FUSE/overlay上のファイル、leaseに対応していないFSのファイルです。並行書込みを排除できず安全に取得できないため読み取りません |
| 文書画面の「読み込みに失敗しました」 | 接続先のserverが止まっているか、`KNOWLEDGE_PLATFORM_API_ORIGIN` が未設定・不正です |

## 既知の制約（Linux）

- WebKitGTK 2.52には、アプリ自身のURL schemeへのBlob／FormData本文でアプリが落ちる不具合があります。shellの初期化scriptで、同一originへ送る本文をページ内で確定してから送ることで回避しています（送る内容は変わりません）。WebKitGTKの更新時は、`mise run desktop:gui:e2e` の文書登録とAPI転送のシナリオで再確認してください。
- 大きな原本（最大256MiB）の取得や登録（最大1GiB）は、shellのメモリ上でいったん全体を保持します。

## Windows実機での確認（依頼者が実施・未実施）

Linuxでのbuild成功やGUI確認は、Windowsでの確認の代わりになりません。現時点では、以下は**いずれも未実施**です。Windows用のbroker実装も未着手のため、ローカルWorkspaceの機能は「利用できません」と表示される状態が正しい挙動です。

準備：

1. Windows 10 Pro／11。WebView2 Runtime（Evergreen）がインストール済みであることと、その版を「アプリと機能」で確認します。同梱・配布はしません。Windows 10 ProのESU（拡張セキュリティ更新）の状況も記録してください。
2. Rust 1.98.1（MSVC。Visual Studio Build Toolsの「C++によるデスクトップ開発」）、Node 24.21／pnpm 12.4.1。
3. `pnpm install --frozen-lockfile`、`pnpm --filter @knowledge-platform/document-web build`、`cargo build --locked --manifest-path apps/desktop/src-tauri/Cargo.toml`。
4. 手元で合成データのserverを起動し、`KNOWLEDGE_PLATFORM_API_ORIGIN=http://127.0.0.1:<port>` を設定してから `apps\desktop\src-tauri\target\debug\knowledge-platform-desktop.exe` を起動します。

確認項目：

1. 既存の文書画面の一覧・詳細、Router・Queryの動作、キーボード操作とfocus、reduced motion（Windowsの「アニメーション効果」設定）
2. Document APIの転送（shell経由の `/v1`）。文書の登録（ファイル添付）と「ファイルを取得」
3. ローカルWorkspace画面が「この実行環境ではローカルフォルダーを利用できません。」と表示し、操作ボタンが出ないこと（fail-closed）
4. ウィンドウが1つだけで、外部URLへ移動しないこと
5. Windows用broker実装後（W1）：フォルダー選択と取消、追加・解除、Workspaceの作成・名前変更、再起動後の復元、限定read（1MiB）とcreate（排他・8MiB）、path traversal、junction・symlink・reparse pointによる外部への脱出、競合中のread、二重操作、結果不明、再起動

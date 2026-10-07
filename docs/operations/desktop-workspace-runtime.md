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
- ダウンロード（「ファイルを取得」）は、Downloadsフォルダーの直下にだけ保存します。同名のファイルがあれば番号を付けて別名にします。OSにダウンロード先の設定が無い場合（Linuxで `user-dirs.dirs` が無い等）は、既にある `~/Downloads` を使います。それも無い場合は保存しません。

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

- 指定できるのは、手元のPCのloopbackのserverだけです（`http://127.0.0.1:<port>` または `http://[::1]:<port>`）。portの指定が必要です（80も `:80` と書けば指定できます）。`localhost`・他のPC・`https://`・末尾のpathは受け付けません（認証の設計がまだ無いため）。形式が違うときは、起動時に標準エラーへ理由を1行出し、画面のAPIは503になります。
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

- 使うもの：tauri-driver 2.1.0（`cargo install tauri-driver --version =2.1.0 --locked`。miseの `[tools]` には入れていません。追跡対象の `mise.lock` が変わり、CIの作業ツリー検査が失敗するため。2.1.0は `--version` を持たないので、report.jsonには実行ファイルのpathとsha256を記録します）、WebKitWebDriver（`webkit2gtk-driver`）、Xvfb、xdotool（OSのフォルダー選択画面を操作）、xclip（選択画面へpathを貼り付ける。`xdotool type` は日本語の文字を落とすことがあり、そのとき選択画面は別名のフォルダーを作ってしまうため）、ImageMagick、Docker（PostgreSQL 18.6）、PDFium（`KP_DSI_PDFIUM_RUNTIME_DIR`、`experiments/document-semantic-inspection/scripts/install-pdfium.sh`）。
- 毎回、使い捨てのPostgreSQLとorganization-server（合成の `sales-01`）を起動し、合成文書を登録してから確認します。HOME・設定・データは実行ごとの一時フォルダーに分けます。Xvfbは空いている画面番号を自分で選び（`-displayfd`）、既存のX serverには接続しません。終了時（Ctrl-C・SIGTERMを含む）は、自分が作ったcontainerとprocessだけを止めます。
- 結果は `apps/desktop/e2e/.state/run-*/report.json` とスクリーンショットに残ります（git管理外）。`status` は、全シナリオが成功したときだけ `passed` です（失敗・準備の失敗は `failed`、名前で絞った実行などで未実行が残れば `incomplete`、中断は `interrupted`）。`qualifying` は、`passed` で、かつ絞り込み無し・作業ツリーがcommit済み・実行ファイルが元のsourceより新しいときだけ `true` になります。証拠として引用できるのは `qualifying: true` の実行だけです。
- 画面からの操作で確かめる項目と、ページのscriptからbrokerのIPCを直接呼ぶ項目（不正な要求、同じ操作IDの再送など）があります。後者はreport.jsonで「IPC：」と明記しています。
- これはLinuxでの証拠です。Windows・WebView2の証拠にはなりません。

2026-10-07の確認結果（24シナリオ。実行記録は下の「証拠とした実行」）：

| シナリオ | 主な確認内容（「IPC」はページのscriptからbrokerを直接呼んだ確認） |
|---|---|
| 起動と既存画面 | ウィンドウ1つ、文書一覧に実APIの合成文書、詳細への移動と「戻る」 |
| Router・Query | `/tasks` を直接開く（SPAのfallback）と再読み込み、タスク一覧にWork APIの内容、検索（PoCの未実装表示）・担当と委任・文書・編集作業への移動 |
| キーボードとfocus | 最初のTabで「メインコンテンツへ」、focus表示、Enterでmainへ、ダイアログの開閉とfocusの戻り |
| reduced motion（2件） | 既定では動きあり。GTKの「アニメーション無効」設定で `prefers-reduced-motion` が成立し、motion tokenが0msになる |
| API転送と境界 | 約2.8MBのmultipart上りと下りのbyte列が一致。名乗りheaderは無視、nosniff、Set-Cookie無し。`/v1/../` はWebKit自身が正規化してアプリのHTMLになり、正規化されない `..%2f`・`%2e%2e%2f`・`..%5c` は `/v1` の内側の404に留まる。OPTIONSは405。他originへのfetchは、CORSを許可したloopbackのserverに対しても `no-cors` でも送信前にCSP（connect-src）で止まり、server側に到達記録が無い。JavaScriptとして登録した原本も `/v1` 応答は `application/octet-stream`＋CSP sandboxで、`<script>` で読み込んでも実行されない。「ファイルを取得」でDownloadsへ保存 |
| 画面からの文書登録 | ファイルを選んで「下書きとして登録」→詳細画面、APIから取り出した原本が一致、編集作業の一覧に表示（Queryの再取得） |
| 画面からの文書編集 | 「メタデータを編集」→保存（PATCH）→APIで値を確認。「作業版を編集」で原本を差し替えて保存（multipart PUT）→APIから同じ内容を取得 |
| ローカルWorkspace | 作成・名前変更、管理フォルダーへの作成と表示、同名・不正な名前（`../`、`CON`、`a/b`）の拒否（`escape.txt` はhome全体に無い）。連続2回の送信でも作成は1回（IPCも1回）、「作成する」の実際の二重クリックでもWorkspace作成は1回 |
| フォルダー選択 | 実際のGTKの選択画面で、取消（変更なし、focusはページに残る）・選択・同じフォルダーの再追加の拒否。IPC：表示中の2つ目の選択要求はpicker_busy |
| 閲覧と制限 | 一覧、階層移動、symlinkは一覧に出さない、hardlinkは開かない。別プロセスが書き込みで開いているファイルは「操作中に内容が変更されました」で読まず、閉じれば読める。1MiBを超えるファイルは、表示内容がファイルの先頭1,048,576バイトと一致 |
| 不正な要求と範囲 | IPC：`..`・絶対path・偽ID・未知のcommand・余分なfield・plugin（fs・shell・window・webview）を拒否、symlink配下は `symbolic_link` で拒否、読み取りの同時5件目は `too_many`、offset 1MiBからの読み取りが残りと一致、1MiB超の範囲は `too_large`。画面：新しいウィンドウ・他originのiframe／`data:` iframe・他originへの遷移は、いずれもloopbackのserverに要求が届かずアプリに留まる |
| 8MiBの上限 | 内容欄に8MiBちょうどを入れて作成→ディスク上も同じ8,388,608バイト。8MiB+1バイトは画面がIPCを送らずに拒否。IPC：画面を通さない8MiB+1バイトはbrokerが `too_large` で拒否。9MiBのファイルは読み取りを拒否、8MiBのファイルは先頭1MiBを表示 |
| 結果不明（応答の消失） | brokerが完了した後でIPCの応答だけを失わせる。Workspace作成：「結果を確認できませんでした」、名前欄の固定、Escapeで閉じない、「あとで確認する」、他Workspaceへの移動の停止、画面を移動して戻るとダイアログを再表示、「結果を確認」は同じ操作IDで再送して作成済みとして確定（Workspace・管理フォルダーとも1つだけ）。ファイル作成：同じく入力・フォルダー移動を止め、「結果を確認」で同じ操作IDのまま確定し、「既にあります」にならない |
| 置き換え・脱出・解除 | 一覧の後でsymlinkに差し替えた階層は開かない、フォルダー自体の置き換えを検出して古い一覧は出さない、解除しても中身は残る |
| 再起動と再送 | 再起動後もWorkspace・名前・追加フォルダー・管理フォルダーの内容を復元。IPC：同じ操作IDの再送は同じ結果（ファイルは1つ）、内容が違えば拒否、同時の同一操作は1つに収束 |
| 強制終了（SIGKILL） | 8MiBの作成中にアプリをSIGKILL→再起動後もWorkspaceと追加フォルダーを復元。IPC：同じ操作IDで再送すると1件・8MiB完全な内容に収束し、余分なファイルは残らない |
| 多数の項目 | 230件のフォルダーで、100件・100件・30件のページ送りと「前の100件」、重複・欠落なし |
| 利用中のフォルダーの解除 | 解除の取消（解除しない、focusが「解除」へ戻る）、解除（中身はそのまま）、管理フォルダーには「解除」が無い。IPC：解除したIDでは読めない、管理フォルダーの解除は `managed_binding` |
| 二重起動 | 2つ目のアプリは「デスクトップ版が別に起動しています。」と表示し、1つ目は使い続けられる |
| ダウンロード先の設定が無い端末 | `user-dirs.dirs` が無いHOMEでも、原本は `~/Downloads` に保存され、作業フォルダーには保存しない |
| backend停止・接続先の設定 | backend停止中は文書画面が失敗を表示し、ローカルWorkspaceは使える（`/v1` は502）。未設定は503と起動時の1行。loopback以外（`localhost`）やpath付きの接続先は形式を案内する503と起動時の1行で、その接続先へは何も送らない |

### Linuxで未確認（この確認の範囲外）

- 安全に読み取れない場所（他のユーザーが所有するファイル、network・FUSE・overlay上のファイル）で「安全に読み取れる状態を確認できない」になること。broker側は設計どおり拒否しますが、実GUIでは試していません。
- 読み取り中に別のプロセスが書き込みを始めた場合（leaseの解除）。確かめたのは「既に書き込みで開かれている」場合だけです。
- 一覧の後で同名の別ファイルへ差し替えた場合の読み取り（brokerの統合試験だけ）。
- ダウンロード名の重複回避（`名前 (n).拡張子`）と、自アプリ以外のblobの拒否（shellの単体試験だけ）。drag&dropの無効化。
- shellの403（Origin/Refererの不一致・main window以外）、413（要求が大きすぎる）、502（応答が大きすぎる）、504（時間切れ）。ページからは作れないため、shellの単体試験だけです。DELETEの転送も、DELETEを使うAPIが今は無いため単体試験だけです。
- フォルダー選択画面でsymlinkを選んだ場合、選択画面を開いたままの終了・強制終了。
- `workspace.recover` を使う画面（画面にはまだ無く、IPCでだけ確認）。
- キーボードだけでのフォルダー追加・解除・ファイル作成、ダイアログ内のTab移動の閉じ込め。
- 既存の文書詳細のその他の操作（公開・履歴・比較・アクセス設定）とDocumentHomePageのフォルダー操作。既存のbrowser E2Eの対象で、このデスクトップ確認では扱っていません。
- macOSでの実行。

### 証拠とした実行

- 2026-10-07 09:12–09:16 UTC、`run-43HUha`：commit `baad3bc`（作業ツリーはcommit済み、絞り込み無し）で24シナリオ・197項目がすべて成功し、`qualifying: true`。そのうち「IPC：」の項目は35です。
- 実行ファイル（debug build）のsha256は `2e9807218607569ee2aab14de9e7e874331a716d9318d14b6494f84a81ae76de`。WebKitGTK 2.52.6、webkit2gtk-driver 2.52.6-0ubuntu0.24.04.1、xvfb 2:21.1.12-1ubuntu1.8、xdotool 1:3.20160805.1-5build1、tauri-driver 2.1.0（sha256 `628e1b01729825cf688858699fb66969e987d04571a615c1c74a8141b34a2b5d`）、Node 22.22.0、PostgreSQL 18.6。
- 強制終了は8MiB中4MiBを書いた時点で起き、同じ操作IDの再送で完全な1件に収束しました。
- 証跡（report.jsonとスクリーンショット）はクラウド環境の `apps/desktop/e2e/.state/`（git管理外）にあり、環境の終了とともに消えます。手元で再現するときは `mise run desktop:gui:e2e` を実行してください。

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
- `/v1` の応答は、JSON・テキスト・octet-stream・PNG/JPEG/GIF/WebP以外の型を `application/octet-stream` にし、CSP sandboxを付けて返します（登録された原本がアプリのコードとして動かないようにするため）。今の画面は原本をblobとして取得するだけなので影響はありません。原本（PDF・HTML・SVG等）を画面内に直接表示する機能を追加する場合は、この規則と合わせて設計してください。
- Linuxのフォルダー選択画面は、main windowの子ウィンドウになりません（rfdのGTK3実装の制約）。選択画面を開いている間は、main windowを閉じる操作も選択画面を閉じるまで待たされます。画面の操作やAPIは動き続けます。

## Windows実機での確認（依頼者が実施・未実施）

Linuxでのbuild成功やGUI確認は、Windowsでの確認の代わりになりません。現時点では、以下は**いずれも未実施**です。Windows用のbroker実装も未着手のため、ローカルWorkspaceの機能は「利用できません」と表示される状態が正しい挙動です。

準備：

1. Windows 10 Pro／11。WebView2 Runtime（Evergreen）がインストール済みであることと、その版を「アプリと機能」で確認します。同梱・配布はしません。Windows 10 ProのESU（拡張セキュリティ更新）の状況も記録してください。
2. Rust 1.98.1（MSVC。Visual Studio Build Toolsの「C++によるデスクトップ開発」）、Node 24.21／pnpm 12.4.1。
3. `pnpm install --frozen-lockfile`、`pnpm --filter @knowledge-platform/document-web build`、`cargo build --locked --manifest-path apps/desktop/src-tauri/Cargo.toml`。
4. 手元で合成データのserverを起動し、`KNOWLEDGE_PLATFORM_API_ORIGIN=http://127.0.0.1:<port>` を設定してから `apps\desktop\src-tauri\target\debug\knowledge-platform-desktop.exe` を起動します。

確認項目：

1. 既存の文書画面の一覧・詳細、Router・Queryの動作、キーボード操作とfocus、reduced motion（Windowsの「アニメーション効果」設定）
2. Document APIの転送（shell経由の `/v1`）。文書の登録（ファイル添付。WebView2は送信時にOriginヘッダーを付けるため、403にならず登録できること）、メタデータの保存、作業版の原本の差し替え、「ファイルを取得」（Downloadsフォルダー直下に保存されること）
3. ローカルWorkspace画面が「この実行環境ではローカルフォルダーを利用できません。」と表示し、操作ボタンが出ないこと（fail-closed）
4. ウィンドウが1つだけで、外部URLへ移動しないこと。自アプリに似たURL（`https://tauri.localhost/`、`http://tauri.localhost:8080/`、`http://user@tauri.localhost/`）へ移動しようとしてもアプリに留まること（開発者ツールは無いので、Linuxの確認と同じくWebDriverかページ内のリンクで確認）
5. `KNOWLEDGE_PLATFORM_API_ORIGIN` を未設定、または `http://localhost:<port>`・末尾path付きにしたとき、文書画面が失敗を表示し、ローカルWorkspaceは使えること（debug buildでは起動時のconsoleに理由が1行出ます）
6. JavaScriptとして登録した原本が、画面から読み込まれても実行されないこと（Linuxと同じ確認。`/v1` 応答が `application/octet-stream` になる）
7. Windows用broker実装後（W1）：フォルダー選択と取消（選択画面がmain windowの子になること）、追加・解除、Workspaceの作成・名前変更、再起動・強制終了後の復元、限定read（1MiB）とcreate（排他・8MiB）、path traversal、junction・symlink・reparse pointによる外部への脱出、Windowsの予約名・ADS・末尾の `.`／空白・`\\?\` 形式、競合中のread、二重操作、結果不明、二重起動

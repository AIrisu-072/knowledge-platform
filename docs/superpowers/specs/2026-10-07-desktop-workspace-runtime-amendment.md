# Desktop Workspace Runtime：Runtime Contractの実装差分

日付：2026-10-07 UTC。状態：**実装と同時に記録**。凍結済み設計の意味を変えずに、ローカルWorkspaceを実装するための差分だけを記録します。

## 根拠と範囲

- 正本：[Domain/API設計 §11–12](2026-10-02-organization-client-v0-domain-api-design.md)（Workspace/ResourceBindingとBounded Runtime Contract）、[Product/UX設計 §11](2026-10-02-organization-client-v0-product-ux-design.md)、[UI設計 §5・§7](2026-10-02-organization-client-v0-ui-design.md)。
- 依頼者の2026-10-07の依頼は、ローカルRuntime・broker・永続化・Runtime Contract adapterの実装を明示しています。対象は、論理Workspaceの作成と名前、managed root、明示的なフォルダーbindingと解除、再起動後の復元です。この差分はその依頼の範囲に限られます。
- 対象外（Organization担当）：server側のWorkspace API（`POST /workspaces`等）、policy-derived binding、業務認可、Task/Workflow/Evidence。
- 対象外（他担当）：Documentの業務semantics、DocumentHomePageのフォルダー操作、Searchの内部。

## 差分1：ローカル専用の論理Workspaceをruntimeが保持する

設計では、server側で論理Workspaceを作り、runtimeがmanaged rootを作る2段階構成です（§11）。server側のWorkspace APIはまだありません。また、Organization担当の領域なので、ここでは作りません。そこで今回は、`scope = principal_device_local` のWorkspaceに限り、broker自身が論理Workspaceを保持します。

| 設計（§12） | 今回の実装 | 利用側への影響 |
|---|---|---|
| `createWorkspace(context, operationId)`：server発行のworkspaceIdにmanaged rootを作る | 予約し、未実装。代わりに `createLocalWorkspace(name, operationId)` でローカルWorkspaceとmanaged rootを一括作成する | server APIが用意された時点で `createWorkspace` を追加する。ローカルWorkspaceはserverへ送らない |
| （なし） | `listWorkspaces()` / `renameWorkspace(context, name, operationId)` | 再起動後の復元と名前変更に使う。名前は表示専用で、pathには使わない |
| `recoverWorkspace(operationId)` | そのまま実装（ready/pending/not_found/unavailable/outcome_unknown） | 応答を失った場合は同じ操作IDで回復する。別のrootは作らない |

`ContextRef.effectiveContextRevision` は、bindingの集合が変わったとき（追加・解除）だけ進みます。名前の変更では進まないため、閲覧中のhandleは名前変更で無効になりません。

## 差分2：応答値の追加（いずれも追加のみ）

- `attachDirectory` は `BindingReceipt` に加えて、更新後のWorkspaceを返します。`detachDirectory` は `void` の代わりに、更新後のWorkspaceを返します。
- `EntryPage.omittedCount`：symlink・特殊ファイル・非UTF-8名・Windowsで無効な名前など、表示できない項目の数です。「全件表示した」と誤解させないために返します。
- `BindingSummary.label`：利用者が選んだフォルダーの**末尾名だけ**です。表示専用で、locatorにもpathにもなりません。serverやAgentへは送りません。
- 失敗は `RuntimeFailure{code, reason?}` です。`code` は設計どおり9種、`reason` も閉じた集合です。OSの生の文言やpathは含めません。

## 差分3：IPCの形

- desktop shellが公開するIPC commandは `local_workspace_runtime` の1つだけです。引数は `{ command, request }` で、`command` に使えるのは次の13種だけです。

  `capabilities`, `workspace.list|create|recover|rename`, `directory.choose|attach|detach`, `entries.list`, `file.openRead|read|closeRead|create`

  上記以外（shell、fs plugin、process等）は `unavailable/unknown_command` として拒否します。
- `request` はcamelCaseで、未知のfieldを受け付けません（path・権限flag・追加optionは渡せません）。bytesはpadding付きBase64です。
- command一覧は、Rust（`crates/local-workspace-runtime/src/wire.rs` の `COMMANDS`）とTS（`apps/document-web/src/runtime/desktop-runtime.ts`）の両方を試験で照合して固定しています。

## 差分4：安全な読み取り（snapshot）の取得方法

設計は、同じinodeへの並行書込みで混在したsnapshotを返さないことを求めています。安全な取得ができない場合は `unavailable/conflict` にすることも求めています。

- **Linux**：まず `fstatfs` でローカルFSの許可一覧（ext2/3/4・xfs・btrfs・tmpfs・f2fs・vfat・exfat・ntfs3・zfs・bcachefs）にあることを確認する。network/FUSE/overlayは遠隔や下層からの書込みをleaseで排除できないため `unavailable/safe_capture_unavailable`。次にread lease（`F_SETLEASE F_RDLCK`）を取得できたとき（他プロセスが書込みopen中でない）だけ、lease保持中に2回全体を読み、statx（btime・ナノ秒mtime/ctime・size・nlink）を3回比較して一致した場合に返す。lease保持中は他プロセスの書込みopen/truncateがkernelで待たされる。lease-break通知はSIGURG（既定動作は無視。shellがSIGURG handlerを入れる場合はこの通知も受ける）。所有者でない等でleaseを取れない場合も `unavailable`。二重読込み＋stat比較だけでは途中停止したwriterの混在状態を返す反例が12回中3回出たため、lease方式に変更した（修正後の反復試験で混在0件）
  - 根拠：二重読込みとstat比較だけでは、途中で止まったwriterの混在状態を返す反例が12回中3回出ました。lease方式に変えた後は、繰返し試験で混在は0件です。
- **macOS等**：書込みを強制的に排除する手段が無いため、読み取りは `unavailable` です。
- **Windows**：未実装のため、全操作がfail-closed（`unavailable/unsupported_platform`）です。実装予定の方式は次のとおりで、Windows実機で検証するまで有効にしません。
  - 親ディレクトリをFILE_SHARE_DELETEなしで保持する
  - `FILE_FLAG_OPEN_REPARSE_POINT` で開き、reparse属性とfile IDを確認する
  - 共有モードで書込みを拒否してsnapshotを取る

## 差分5：desktop shell（Tauri v2）の境界と転送（2026-10-07追記）

依頼者の判断（[判断事項](../../decisions/2026-10-07-tauri-v2-desktop-qualification.md)、全項目合意）を受けて追加しました。

- **window**：main windowを1つだけコードで作ります。`window.open` 等の新しいウィンドウは拒否し、遷移は同梱アプリのURL（Linux/macOS：`tauri://localhost`、Windows：`http://tauri.localhost`）と、同梱アプリが作ったblob URLだけを許可します。drag&dropのOS連携（絶対pathを渡すもの）は無効です。
- **IPC**：capabilityは `main` windowのローカル（同梱）originに `allow-local-workspace-runtime` の1件だけです。Tauri core・pluginの権限は与えません。remote contentはcommandに届きません。
- **転送**：アプリ自身のURL schemeをshellが登録し、同梱assetの配信と `/v1` の転送を行います（Tauri既定のasset handlerは未知のpathにindex.htmlを返すため、置き換えが必要）。
  - 転送先は `KNOWLEDGE_PLATFORM_API_ORIGIN` の1つだけで、literalのloopback（`127.0.0.0/8`・`::1`）・`http`・port必須・path無しに限ります。
  - 正規化した後のpathが `/v1` 以下で、originが同じ場合だけ転送します（`..`・`%2e%2e`・`\`・`//host` での脱出は拒否）。method：GET/HEAD/POST/PUT/PATCH/DELETE。
  - 要求header：`accept`・`accept-language`・`content-type`・`traceparent` だけ。応答header：`content-type`・`content-disposition`・`content-language`・`cache-control`・`etag`・`last-modified`・`retry-after` だけ（Set-Cookie・Location・CORS系は返しません）。`X-Content-Type-Options: nosniff` を付けます。
  - redirectは追わず、cookie・system proxyは使いません。上限：要求本文1GiB＋1MiB、応答本文256MiB＋1MiB、接続5秒、全体180秒。失敗はpath等を含まないproblem（503未設定・502接続不可・504時間切れ・400宛先不正・405 method・413大きさ）。
  - Origin/Refererは、付いていれば同梱アプリと一致することを求めます（WebKitGTKは同一originのcustom scheme要求にOriginを付けないため、必須にはできません）。
  - 既存serverのCORS・認証・identityの扱いは変えません（名乗りheaderは転送しません）。
- **CSP**：既存previewと同等に、Tauri IPCの `ipc:` と `http://ipc.localhost` を `connect-src` に加えたものです。backendへの直接接続はできません。
- **本文の確定（WebKitGTK回避）**：WebKitGTK 2.52はcustom schemeへのBlob/FormData本文でSIGSEGVします。shellは初期化scriptで、同一originへのGET/HEAD以外の要求本文をページ内でArrayBufferに確定してから送ります。bytes・method・header・中断signalは変わりません。他originとIPCには触れません。
- **ダウンロード**：同梱アプリのblob URLを、Downloadsフォルダー直下（WebViewが重複を避けた名前）に保存する場合だけ許可します。
- **picker**：`rfd` の単一フォルダー選択。brokerはworker threadから呼び、lockを持たずに待ちます。LinuxではGTKのmain contextでdialogが動きます。Windowsではmain windowを親にします。

## 不変条件（変更なし）

- 任意の絶対path、shell、実行ファイル起動、汎用FS APIは公開しません。architecture-lintで、`std::process` などをこのcrateから禁止しています。
- 各名前要素を検証します。`.`/`..`、区切り文字・NUL・制御文字、`<>:"|?*`、末尾の`.`/空白、予約デバイス名、ADS、255byte超、深さ32超は拒否します。percentなどの再decodeは行いません。
- 閉じ込めは、`openat(O_NOFOLLOW|O_DIRECTORY)` で1要素ずつ辿ることで行います。文字列prefixでの比較はしません。次のものは拒否します。
  - symlink
  - 2つ以上の場所からリンクされたファイル（hardlink）
  - 特殊ファイル
  - broker自身の状態root
- 作成は排他（`O_EXCL|O_NOFOLLOW`）で、上書きしません。排他作成の直後に自分のファイルidentityを記録してから書き込み、再試行ではそのidentityのファイルだけを採用します（他者が作った同じ内容のファイルは採用しません。自分の書きかけファイルは削除して作り直します）。作成後に親を辿り直し、同じディレクトリであることを確認します。違っていれば自分のファイルを消して `conflict` を返し、削除を証明できない場合は `outcome_unknown` として記録を保持します。
- ファイルは種類（通常ファイル）を確認してから開きます（FIFOのwriterの解放や、特殊ファイルを開く副作用を避けるため）。binding rootは、dev/inoに加えて作成時刻でも照合します。
- registryの変更は複製へ適用し、保存に成功してから採用します。保存に失敗した場合はdiskの状態を読み直します。失敗した変更が、再試行で成功扱いになることも、再起動で巻き戻ることもありません。Workspace作成の操作記録は押し出さないため、同じ操作IDで2つ目のrootが作られることはありません。
- 上限：一覧100件/頁、読み取り1回1MiB、snapshot 8MiB、同時に保持するhandle 4件・合計32MiB・期限5分、作成8MiB、名前255byte、深さ32、明示binding 32件、picker 1件。
- read handleは、close・解除・期限切れ・再起動で無効になります。再起動しても復活しません。

## 利用側への影響

- 既存のDocument・Task・Searchの画面とAPIは変えていません。
- 共通Shellの変更点は次の2つだけです。
  - headerのruntime表示（desktopのときだけ表示し、browserの見た目は変わりません）
  - `activeNavigation` 型への値の追加
- 主ナビゲーション（タスク／文書／検索）は変えていません。ローカルWorkspace画面へは `/local-workspaces` から開きます。
- ローカルWorkspace画面（2026-10-07、実GUI確認で見つけた3点）：表示中のフォルダーで「開く」を押すと一覧を取り直す。一覧の取得に失敗したら古い一覧を表示しない。desktopでruntimeが使えないとき、理由（別に起動中・記録を読めない・未対応OS）を表示する。
- Organization担当がserver側Workspaceを実装する際は、`createWorkspace(context, operationId)` を追加し、server発行のworkspaceIdをローカルの記録と対応付けてください。ローカルWorkspaceを共有Workspaceとして扱わないでください。

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

- **Linux**：read lease（`F_SETLEASE F_RDLCK`）を取ります。取れるのは、書込みopen中のプロセスが無いときだけです。leaseを保持している間は、他プロセスの書込みopenとtruncateがkernelで待たされます。その状態で2回全体を読み、statx（btime・ナノ秒mtime/ctime・size・nlink）を3回比較して一致した場合だけ返します。lease-break通知はSIGURG（既定動作は無視）で受けます。所有者でない・FSが非対応などでleaseを取れない場合は `unavailable/safe_capture_unavailable` です。
  - 根拠：二重読込みとstat比較だけでは、途中で止まったwriterの混在状態を返す反例が12回中3回出ました。lease方式に変えた後は、繰返し試験で混在は0件です。
- **macOS等**：書込みを強制的に排除する手段が無いため、読み取りは `unavailable` です。
- **Windows**：未実装のため、全操作がfail-closed（`unavailable/unsupported_platform`）です。実装予定の方式は次のとおりで、Windows実機で検証するまで有効にしません。
  - 親ディレクトリをFILE_SHARE_DELETEなしで保持する
  - `FILE_FLAG_OPEN_REPARSE_POINT` で開き、reparse属性とfile IDを確認する
  - 共有モードで書込みを拒否してsnapshotを取る

## 不変条件（変更なし）

- 任意の絶対path、shell、実行ファイル起動、汎用FS APIは公開しません。architecture-lintで、`std::process` などをこのcrateから禁止しています。
- 各名前要素を検証します。`.`/`..`、区切り文字・NUL・制御文字、`<>:"|?*`、末尾の`.`/空白、予約デバイス名、ADS、255byte超、深さ32超は拒否します。percentなどの再decodeは行いません。
- 閉じ込めは、`openat(O_NOFOLLOW|O_DIRECTORY)` で1要素ずつ辿ることで行います。文字列prefixでの比較はしません。次のものは拒否します。
  - symlink
  - 2つ以上の場所からリンクされたファイル（hardlink）
  - 特殊ファイル
  - broker自身の状態root
- 作成は排他（`O_EXCL|O_NOFOLLOW`）で、上書きしません。作成後に親を辿り直し、同じディレクトリであることを確認します。違っていれば自分のファイルを消して `conflict` を返します。
- 上限：一覧100件/頁、読み取り1回1MiB、snapshot 8MiB、同時に保持するhandle 4件・合計32MiB・期限5分、作成8MiB、名前255byte、深さ32、明示binding 32件、picker 1件。
- read handleは、close・解除・期限切れ・再起動で無効になります。再起動しても復活しません。

## 利用側への影響

- 既存のDocument・Task・Searchの画面とAPIは変えていません。
- 共通Shellの変更点は次の2つだけです。
  - headerのruntime表示（desktopのときだけ表示し、browserの見た目は変わりません）
  - `activeNavigation` 型への値の追加
- 主ナビゲーション（タスク／文書／検索）は変えていません。ローカルWorkspace画面へは `/local-workspaces` から開きます。
- Organization担当がserver側Workspaceを実装する際は、`createWorkspace(context, operationId)` を追加し、server発行のworkspaceIdをローカルの記録と対応付けてください。ローカルWorkspaceを共有Workspaceとして扱わないでください。

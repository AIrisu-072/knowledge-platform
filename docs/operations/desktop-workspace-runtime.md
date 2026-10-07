# ローカルWorkspace Runtime：操作と検証の手順

対象：同じReact frontendをブラウザーとdesktopで動かすためのRuntime Contractと、端末ローカルのWorkspace broker（`crates/local-workspace-runtime`）。
本番サーバーへの反映、認証・本番データ・外部サービスへの接続は、この手順には含みません。

## いま使えること

| 実行環境 | できること |
|---|---|
| ブラウザー版（既存のpreview・本番Web） | これまでどおり文書・タスク・検索が使えます。`/local-workspaces` を開くと「ブラウザー版ではローカルフォルダーとWorkspaceを利用できません」と表示され、作成・追加などのボタンは出ません。既存画面の見た目は変わりません |
| desktop版（Tauri shell） | **まだ起動できません。** Tauri依存の追加には依頼者の承認が必要です（[判断事項](../decisions/2026-10-07-tauri-v2-desktop-qualification.md)）。Runtime（broker、IPCの形、画面）は実装済みです |
| 検証用の代替host | Chromiumと実brokerを、テスト専用のstdio bridgeでつないだ通し試験だけ（下記）。利用者向けの起動手段ではありません |

desktop版で使える予定の操作（画面とbrokerは実装・試験済み）：

- 「新しいWorkspace」で名前を入れて作成します。端末内の管理フォルダーが自動で作られます。フォルダーの場所はWorkspace名と無関係で、名前を変えても場所は変わりません。
- 「フォルダーを追加」でフォルダー選択画面を開きます。取り消した場合は何も変わりません。選んだフォルダーは参照として登録され、中身は移動もコピーもされません。
- 「開く」でフォルダー内を一覧（100件ずつ）します。下の階層への移動と「上の階層へ」、テキストの内容表示（先頭1MiBまで）ができます。
- 「この場所にファイルを作成」で新しいファイルを保存します（8MiBまで、同名のファイルは上書きしません）。
- 「解除」でフォルダーの登録だけを外します。中身は削除されません。管理フォルダーは解除できません。
- アプリを再起動すると、Workspace・名前・フォルダーの登録が復元されます。開いていた読み取りは復元されません。

安全のため開けないもの：シンボリックリンク・ジャンクション、2つ以上の場所からリンクされたファイル、通常のファイルでないもの、アプリの管理領域、`..` や絶対pathのような指定、Windowsの予約名（CON等）。

## 開発者向けの検証コマンド

```bash
# broker（Linux）：単体12＋統合37＋wire 5
cargo test -p local-workspace-runtime
cargo clippy -p local-workspace-runtime --all-targets --locked -- -D warnings

# frontend：全GUI試験・型・build
pnpm --filter @knowledge-platform/document-web test
pnpm --filter @knowledge-platform/document-web typecheck
pnpm --filter @knowledge-platform/document-web build

# Chromium＋同じReact build＋Desktop adapter＋実broker（テスト専用stdio bridge）
mise run desktop:bridge:e2e
```

CIでは `desktop-runtime-bridge` jobが最後のコマンドを実行し、required-checkに含まれます。
ローカルでPlaywright指定版のChromiumが無い場合は、`KP_CHROMIUM_EXECUTABLE=/path/to/chrome` を指定できます。この場合は版が異なることを結果に記録してください。

## 状態の保存場所と復旧

brokerは、shellから渡された状態フォルダー（Tauriではアプリデータフォルダーを想定）に次のものを保存します。

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

## Windows実機での確認（承認後・未実施）

Linuxでのbuild成功は、Windowsでの確認の代わりになりません。Tauri shellの承認後、Windows 10 Pro／11の実機で、少なくとも次の項目を確認してください。現時点では**いずれも未実施**です。

1. 既存の文書画面の一覧・詳細、Router・Queryの動作、キーボード操作とfocus、reduced motion
2. Document APIの転送（shell経由の `/v1`）
3. フォルダー選択と取消、追加・解除、Workspaceの作成・名前変更、再起動後の復元
4. 限定read（1MiB範囲）とcreate（排他・8MiB）
5. path traversal、junction・symlink・reparse pointによる外部への脱出、競合中のread、二重操作、結果不明、再起動
6. WebView2 Runtimeの有無と版、Windows 10 ProのESU状況

Windows用のbroker実装は、まだfail-closed（すべて「利用できません」）です。実機での検証を伴う実装が先に必要です。

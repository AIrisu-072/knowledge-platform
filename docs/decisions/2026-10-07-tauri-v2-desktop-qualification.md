# Tauri v2 Desktopの資格確認結果と依頼者判断事項

日付：2026-10-07 UTC。状態：**Tauri shellの依存追加・build・実行はSTOP（依頼者の判断待ち）**。Runtime側（broker・Runtime Contract・画面）は実装済みで、Tauri shellを追加すればつながる状態です。

## 確認した事実（metadataのみ。build・実行はしていません）

scratchpadで `tauri = "=2.12.1"` のlockを作り、`cargo tree` でtarget別・edge別に分けて確認しました。cargo-denyやOSVの実行は行っていません（OSVの宛先api.osv.devへは、この環境の通信方針で接続できません）。

| 項目 | 結果 |
|---|---|
| 現在の安定版 | tauri 2.12.1（crates.io index、2026-10-07時点。3.0はalphaのみ）、@tauri-apps/api 2.12.1（Apache-2.0 OR MIT、依存なし） |
| Windows実行バイナリに入るMPL-2.0 | `option-ext 0.2.0`（dirs 7 → dirs-sys 0.5 経由） |
| Windowsのhost build（proc-macro・build script）に入るMPL-2.0 | `cssparser 0.37.0`、`selectors 0.38.0`、`dtoa-short 0.3.5`、`cssparser-macros 0.7.1`（tauri-codegen/tauri-utils → dom_query 経由） |
| Linuxだけに入るadvisory対象 | `glib 0.18.5`（RUSTSEC-2024-0429）、gtk3-rs一式（gtk/gdk/atk 0.18、unmaintained）、`proc-macro-error 1.0.4`（RUSTSEC-2024-0370、host） |
| lockfileへの影響 | Cargo.lockはtargetに関係なく全依存を含みます。Tauriを含むlockをcommitした時点で、既存の `osv-scanner scan source -r .` がglibを検出します。root workspaceに入れれば、cargo-denyのlicense（MPL-2.0は許可外）も失敗します |
| 既存policy | `spec/architecture/dependency-rules.toml` は `native_windows_supported = false`、`allow_native_windows = false` です |
| この環境 | webkit2gtk-4.1が無く、Linuxでの実行はできません（導入はしていません） |
| WebView2 | Windows 10/11のEvergreen Runtimeを前提とします。配布・同梱する場合はMicrosoftのRuntime規約が適用されます（PR52で別STOPとして記録済み） |
| Windows 10 Pro | 通常サポートは2025-10-14に終了しました。消費者向けESUは2026-10-13までで、検証時点の残りは6日です。商用ESUの有無で前提が変わります |

## 既存の承認と、今回使わなかったもの

- PR51：`cssparser 0.37.0`／`selectors 0.38.0`／`cssparser-macros 0.7.0` について、**host-buildの適格性確認だけ**を承認済みです。現在の版は `cssparser-macros 0.7.1` でずれています。`dtoa-short`／`option-ext` は判断待ちです。
- PR52：`glib 0.18.5`／`proc-macro-error 1.0.4` の限定例外です。対象は、2026-10-10 00:00 UTCまでの、固定された「buildしない依存一覧の検証」だけです。Linuxでの利用、恒久的なignore、実runtimeの実行は対象外です。**今回はこの例外を使っていません。**
- OSVへの依存情報送信の範囲は、PR52で依頼者の確認待ちです。今回の変更は外部依存を追加していないため（Cargo.lockの差分は新crate自身の1項目だけ）、CIが送る依存情報はmainと同じです。

## 依頼者に判断していただきたいこと

Tauri shell（`apps/desktop/src-tauri`）を追加・build・実行するには、次の承認が必要です。

1. **MPL-2.0の例外**：`option-ext 0.2.0`（実行バイナリに入る）と、host buildの4件（`cssparser 0.37.0`、`selectors 0.38.0`、`dtoa-short 0.3.5`、`cssparser-macros 0.7.1`）を、desktop用lockに限ってproduction buildで許可するか。注意（NOTICE）文書の同梱が必要になります。
2. **Linux専用のadvisoryの扱い**：Windows専用のdesktop targetとし、lockに含まれるLinux専用crate（glib/gtk3-rs/proc-macro-error）について、「Windowsのbuild graphに入らないこと」を機械検査で確認したうえで、desktop lockに限ったOSV/cargo-deny例外を設けるか。あるいはLinuxでの利用も承認するか。
3. **Windowsでの検証経路**：Windows CI（`allow_native_windows` の改定）を認めるか、依頼者のWindows実機での手動検証とするか。
4. **WebView2 Runtime**：同梱・bootstrapperによる配布の有無と、規約への同意。推奨は「同梱なし（端末にインストール済みのEvergreenを使う）」です。
5. **OSV送信**：desktop lockの公開依存名と版をapi.osv.devへ送ることを認めるか。

承認をいただければ、shellは次の構成で追加します。broker・IPC・画面は実装済みです。

- 単一main window。remote contentにはcommandを渡しません。CSPは既存previewと同等にします。
- IPC commandは `local_workspace_runtime` だけを登録します。picker（`rfd` 想定、tauri-plugin-dialog/fsは使いません）は単一フォルダー選択だけです。
- `/v1` はshellのcustom protocolから、設定された1つのbackendへ転送します。既存serverのCORSは広げません。

## 証拠として扱わないもの

- このcrateの `cargo check --target x86_64-pc-windows-gnu` が成功したことは、Windowsでの動作の証拠ではありません（Windowsはfail-closedのstubです）。
- Chromium上の通しE2E（テスト専用stdio bridge）は、Tauri・WebView2・Windowsの証拠ではありません。

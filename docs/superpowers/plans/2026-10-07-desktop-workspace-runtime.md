# Desktop Workspace Runtime：実装計画（最小単位）

日付：2026-10-07 UTC。基点main `d515aa38085c9ed7e41f8103d9c1a6c576025fd4`。branch `claude/upbeat-tesla-qcu94x`。
設計：[Domain/API §11–12](../specs/2026-10-02-organization-client-v0-domain-api-design.md)、[実装差分](../specs/2026-10-07-desktop-workspace-runtime-amendment.md)。

## 単位と完了条件

| # | 単位 | 内容 | 完了条件 | 状態 |
|---|---|---|---|---|
| R1 | broker core | `crates/local-workspace-runtime`：registry、論理Workspace、managed root、選択と追加・解除、locator検証、handle相対のlist/read/create、lease付きsnapshot、操作IDの冪等性 | テストを先に書いてREDを確認→GREEN。clippy、architecture-lint、macOS/Windowsの`cargo check` | 完了 |
| R2 | wire | 単一IPC command、13種のcommand、未知fieldの拒否、Base64 | wire試験4件、TSとのcommand一覧照合 | 完了 |
| R3 | frontend contract | 型、Browser/Desktop adapter、起動時の選択、Provider | adapter試験6件、host bridgeの参照制限の試験 | 完了 |
| R4 | 画面 | `/local-workspaces`、Shell headerの表示（desktop時だけ） | 画面試験10件、全GUI、型、build、既存mock E2E | 完了 |
| R5 | 通しE2E | テスト専用stdio bridge＋Chromium | 6件、CI job（required-check） | 完了（hosted確認はPRで行う） |
| T1 | Tauri shell | `apps/desktop/src-tauri`（独立Cargo workspace）、単一window、IPC 1種、picker、`/v1`転送、desktop用の依存policy | 依頼者の承認（2026-10-07、全項目合意）、単体試験と変異試験、clippy（Linux・Windows gnu）、cargo deny、architecture-lint | 完了 |
| T2 | 実GUI確認 | tauri-driver＋WebKitWebDriver＋Xvfb＋xdotool＋実backend（`mise run desktop:gui:e2e`、CIにはしない） | 16シナリオ・99項目がLinuxで成功。Windows・WebView2の証拠にはしない | 完了（Linux） |
| W1 | Windows broker | handle相対、reparse拒否、share-deny-write snapshot | Windows実機での検証 | 未着手 |
| W2 | Windows実機確認 | 依頼者の実機で、[手順](../../operations/desktop-workspace-runtime.md)の確認項目1〜4（W1の後に5） | 実機の記録 | 未実施（依頼者） |

## 守ること

- Organizationの業務API、policy-derived binding、DocumentHomePageのフォルダー操作、Searchの内部は変更しません。
- root workspaceの外部依存は増やしません（R1〜R5のCargo.lock差分は新crate自身の1項目だけ。Tauri等はdesktop用の独立lockだけに入れる）。
- PR52の限定例外は使いません。Linuxのbuild成功やChromiumでの試験を、Windows・Tauriの証拠として扱いません。

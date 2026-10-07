# Desktop Workspace Runtime：実行状況

## 2026-10-07 UTC — Runtime側の実装と検証（Tauri shellは判断待ち）

- 基点main：`d515aa38085c9ed7e41f8103d9c1a6c576025fd4`（引継ぎ時と同じで、作業開始時に再確認済み）。branch：`claude/upbeat-tesla-qcu94x`。計画は[こちら](../plans/2026-10-07-desktop-workspace-runtime.md)、契約差分は[こちら](../specs/2026-10-07-desktop-workspace-runtime-amendment.md)。
- 既存成果の区別：
  - mainに統合済み：Organization Product/Domain API/UI設計とPhase1–3の凍結、Browser PoC、preview、gitleaksの4 fingerprint。
  - mainに無い：PR50〜53は未統合のDraftです。内容は文書と、buildしない資格確認用のlockだけです。#52・#53のsecurity失敗は、資格確認lock内のglib/proc-macro-errorをOSVが検出したことによるもので、未解消です。
  - 未公開の候補：`7d64603`/`b4c221a` はrepositoryに実体がなく、再利用できません。
  - PR50〜53の内容は今回のPRに取り込んでいません。Tauri資格確認の結論は[判断事項](../../decisions/2026-10-07-tauri-v2-desktop-qualification.md)へ新しく記録しました。
- 完了（R1–R5）：
  - broker
  - wire（単一IPC・13 command）
  - frontend contract/adapter/Provider
  - `/local-workspaces` 画面
  - テスト専用stdio bridgeによるChromium通しE2Eと、CI job `desktop-runtime-bridge`
- TDDの記録：
  - brokerの統合試験31件と、wire試験4件は、stub実装に対してREDを確認してから実装しました。
  - 並行書込みでの混在snapshotは、実装後の反復試験で反例が出ました（12回中3回）。新しい試験でREDを再現（6回中2回）してから、read lease方式に修正しました。
  - inode番号の再利用で置換を見逃す反例も見つかり、statxのbtimeを識別子に含めて修正しました。
  - 作成途中のcrashからの復旧試験2件は、実装の後に追加した回帰試験で、初回からGREENでした。
- ローカル検証（Linux、Node 22.22／pin版はNode 24.21）：
  - broker：単体10、統合31、wire 4、合計45件PASS。clippy -D warnings、fmt、architecture-lint PASS（禁止patternの陰性確認も実施）。
  - `cargo check --target x86_64-apple-darwin|x86_64-pc-windows-gnu` は警告0です。
  - 競合系試験は10回連続PASS。
  - 全GUI：59 suites／1439件PASS（mainの1421件に新規18件）。型検査（app、document-poc-runtime、organization-runtime、desktop-bridge）、本番build（既存種別の性能警告3件）、preview試験36件。
  - 既存mock E2E 6件PASS。事前導入済みChromium 1194で実行しており、pin版1243ではありません。
  - desktop-bridge E2E 6件が3回連続PASS（同じChromium 1194）。
- 未検証：
  - Tauri shell（依存の追加・build・実行がSTOP）
  - WebView2
  - Windows 10 Pro／11の実機
  - Windows版brokerの動作。fail-closedのstubだけで、実装も未着手です
  - Document APIのdesktop経由の転送
  - macOSでの実行（`cargo check` のみ）
  - Linuxでの、他人所有ファイルやlease非対応FS上での読み取り（`unavailable` として拒否する設計どおりであることを試験していない）
  - pin版Chromiumでの実行（PRのhosted CIで確認予定）
- 判断待ち（依頼者）：MPL-2.0の例外（5件）、Linux専用advisoryの扱い、Windows CIか実機か、WebView2の規約、desktop lockのOSV送信。詳細は[判断事項](../../decisions/2026-10-07-tauri-v2-desktop-qualification.md)。
- Design Freezeとの差分：[実装差分](../specs/2026-10-07-desktop-workspace-runtime-amendment.md)のとおり。ローカル専用の論理Workspace、追加の応答値、単一IPC、lease付きsnapshot。依頼者の2026-10-07の依頼の範囲内として記録しました。server側のWorkspaceは予約済みで未実装です。
- 次のexact action：独立review → push → Draft PR → hosted CI（`desktop-runtime-bridge` を含む全必須job）→ 受入確認後にmainへ統合 → 統合後のCIを確認。Tauri shellは承認後に `apps/desktop/src-tauri` として追加します。

# Desktop Workspace Runtime：実行状況

## 2026-10-07 07:30 UTC — Tauri shellの実装と実GUI確認（依頼者が判断5項目に合意）

- 依頼者が判断5項目すべてに合意し、「クラウド環境でTauriを構築し、デスクトップアプリのGUIから確認する。CIにはしない」と指示。[判断事項](../../decisions/2026-10-07-tauri-v2-desktop-qualification.md)へ合意内容と適用を記録しました。
- 基点：PR95統合後のmain `04076b1`（PR96を含む）。branch `claude/upbeat-tesla-qcu94x` をmainから作り直しました。main `04076b1` のCI run 37578317667は全job成功で、PR95統合直後（`93947f3`）のrust-test失敗（無関係なDocker pullの切断）は後続mainで解消済みです。
- 実装：`apps/desktop/src-tauri`（独立Cargo workspace、Tauri 2.12.1）。単一window、IPC 1件、rfdのフォルダー選択、URL schemeの置き換えによるasset配信と `/v1` 転送（loopbackの1 originだけ）、遷移・新規window・downloadの制限、desktop用のdeny.toml／osv-scanner.toml、architecture-lintの境界、ローカル専用のmise task（`desktop:check`／`desktop:build`／`desktop:gui:e2e`）。
- 実GUIで見つけて直したもの：
  - WebKitGTK 2.52がcustom schemeへのBlob/FormData本文でSIGSEGV（gdbで確認、文字列・バイト列は正常）→初期化scriptで本文をページ内で確定。
  - WebKitGTKは同一originの要求にOriginを付けない→Origin必須をやめ、「付いていれば一致」の多層防御へ（RED→GREEN）。
  - `<a download>` のblob URLが遷移制限で拒否されていた→自アプリのblob URLだけ許可（RED→GREEN）。
  - ローカルWorkspace画面：同じフォルダーの「開く」で再取得しない／置換検出後も古い一覧を表示／二重起動の理由を表示しない→いずれも画面試験を先に追加して修正。
- 検証（ローカル、Linux）：
  - shell：単体10件、変異4件（転送先・header・loopback・Referer判定）をすべて検出、clippy -D warnings（Linux、`x86_64-pc-windows-gnu`）、fmt、`cargo deny check`（desktop）。
  - root：fmt、`cargo deny check`、architecture-lint、assurance scan/plan/run/report、repo:policyの追跡物検査、gitleaks（今回のcommit範囲0件）。
  - GUI：全64 suites／1557件、型検査、本番build。
  - 実GUI（`mise run desktop:gui:e2e`）：16シナリオ・99項目がすべて成功（tauri-driver 2.1.0、WebKitWebDriver、WebKitGTK 2.52.6、Xvfb、xdotool、PostgreSQL 18.6、organization-server）。証跡は `apps/desktop/e2e/.state/run-*/`（git管理外）。
- 未検証：Windows実機・WebView2・MSVC build（依頼者が実施、[手順](../../operations/desktop-workspace-runtime.md)）、Windows版broker（未実装、fail-closed）、macOSでの実行、OSVの実送信（この環境からapi.osv.devへは接続不可。PRのsecurity jobで確認）。
- 次のexact action：独立review（実行中）の指摘を確認・修正 → push → Draft PR → exact-head CI（特にsecurityのOSV） → 統合 → 統合後のmain CIを確認。

## 2026-10-07 04:20 UTC — 修正commitの再reviewと追加修正

- 修正commit（c5f9564/4df588a）の独立再review：
  - 旧指摘1〜5、10、11は修正済みと確認されました。
  - 6、7、8、13、14は一部修正にとどまり、新しい欠陥が5件見つかりました。そのうち4件は再reviewで実際に再現されています。
- 再現された新規欠陥（N1、N2、N4、N5）と、上限処理の問題（N3）の修正（3446677/af55c22）。いずれもREDの反例試験を先に追加しています。
  - N1（Medium）：再試行が、利用者による同じファイルの上書き編集を消し得ました。意図bytesの真の接頭辞（自分の書込み途中）である場合だけ作り直し、それ以外はconflictとして保持します。
  - N2（Medium）：結果不明のまま解除できたため、画面が固着しました。未確定の間は解除・追加・名前変更を無効にします。
  - N3（Low）：保留中の作成予約に上限が無く、直前に追加した記録が押し出されることがありました。予約を16件に制限し、記録の上限を1024件へ広げ、直前に追加した記録は押し出しません。
  - N4（Low–Medium）：再確認が `stale_context` になると、操作を破棄していました。操作IDを保持し、Workspaceを更新して再確認します。
  - N5（Low）：作成ダイアログを閉じられませんでした。「あとで確認する」を追加し、一覧から同じ入力で再開できます。
  - 応答の照合を追加しました：名前変更・解除の応答のWorkspace、解除済みのbinding、回復receiptの操作ID。
- 残存事項（記録のみで、このPRでは直していません）：
  - FreeBSD等ではerrnoを消去できず、一覧がfail-closedになります。
  - 種類を確認してから開くまでの間にFIFOへ差し替えられた場合の、短い競合（Linuxでは `O_PATH` 化で解消できます）。
  - musl版ではbtimeが無く、識別子の強化が効きません。
  - 作成直後、identityを記録する前にcrashすると、空ファイルが残ります。再試行は `AlreadyExists` になります。`O_TMPFILE`+`linkat` 化が候補です。
  - 作成receiptの `sha256` は形式だけを確認しています。
- 修正後のローカル検証：
  - broker：単体12、統合37、wire 5、合計54件。clippy、fmt。
  - GUI：全59 suites／1444件、型検査、本番build。
  - 既存mock E2E 6件、desktop-bridge E2E 6件（Chromium 1194）。
- 次のexact action：push → 新headの全必須CIを確認 → Draft解除と統合を判断 → 統合後のmain CIを確認。

## 2026-10-07 04:00 UTC — 独立reviewの指摘を修正（PR95）

- [PR95](https://github.com/AIrisu-072/knowledge-platform/pull/95)（Draft）。初回head `592ba99` のCI run 37567267684では、rust-testを除く全job（security、policy、rust-static、desktop-runtime-bridge、document-poc-runtime、portability-macos等）がSUCCESSでした。rust-testは実行中のまま、次のpushで置き換えました。
- 独立review（`d515aa3..5dbca37` を対象）で、実際に再現された欠陥が見つかりました。いずれもREDの反例試験を先に追加してから修正しています。
  - Important：registryの保存に失敗したとき、解除・追加・名前変更がメモリにだけ反映され、再試行で成功扱いになり、再起動で巻き戻った。
  - Important：操作記録が上限で押し出されると、同じ操作IDで2つ目のWorkspaceとmanaged rootが作られた。
  - 修正済み：binding外へ移された親フォルダーへの作成で、ファイルが孤立して残った（592ba99）。
  - Pending状態の作成の再試行で、contextを検査せず、他者の同一内容ファイルを採用し得た。
- 再現前の指摘（suspected/Minor）のうち、PR範囲内のものも修正しました。
  - network/FUSE/overlay上でのlease（ローカルFSの許可一覧に限定）
  - readdirのerrorを終端と区別していなかった
  - FIFO/特殊ファイルを種類確認の前に開いていた
  - binding rootの識別にbtimeを追加
  - 古いcontextのhandleが上限枠を占有していた
  - pickerがpanicした場合のticket解放
  - 画面移動で結果不明の操作が失われた（QueryClient単位のstoreで保持し、確定まで移動を止める）
  - IPC応答の照合（offset／世代／長さ／eof、receiptの操作ID／ref／size、一覧の親locator）
  - recoverWorkspaceをmutationとして扱う
  - SIGURGの扱いは文書に明記しました。
- 修正後のローカル検証：
  - broker：単体11、統合37、wire 5、合計53件。clippy、fmt、architecture-lint、macOS/Windowsの `cargo check`。競合系6件は8回連続PASS。
  - GUI：全59 suites／1441件、型検査4構成、本番build。
  - 既存mock E2E 6件、desktop-bridge E2E 6件（いずれもChromium 1194、新しいbuild）。
- 次のexact action：push → 新headの全必須CIを確認 → Draft解除と統合を判断 → 統合後のmain CIを確認。

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

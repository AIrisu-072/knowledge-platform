# text/PDF 原本ビューアの実行状況

2026-10-08 UTC。承認済みtext+PDF方針、既読はdetail正常表示trigger維持。branch `feat/document-viewer-20261008`、基点6440e026からPR108統合main `6a34de3f0904949daca304b9178ef5125ea13d82`へfast-forward。両commitのtree差分は空。directoryはクラウド親所有、原本追加・一括登録は別worktree所有。upload/API schema/Directory/実アカウント設定は変更していない。

計画: [原本ビューア](../plans/2026-10-08-document-original-viewer.md)。依存判断: [PDF依存と実行境界](../../decisions/2026-10-08-document-original-viewer-dependency.md)。操作: [原本を画面で確認する](../../operations/document-original-viewer.md)。設計追補のPROPOSEDから、ユーザーが2026-10-08 06:23:37 UTCに親へ承認したtext+PDF部分を実装する。新directory設計はこの承認へ混ぜない。

## 実装

- 現行版の原本一覧から、選択したtext/PDFを認可・Audit付き既存binary APIで表示。10MiB受信上限、UTF-8厳格decode、textnode表示。全量Blob fallbackは通常download用途のまま、viewerだけstreamで実bytesを制限。
- PDF.js固定6.4.299、Apache2/MIT notice、同一origin moduleworkerを明示portで接続。CSP不変、object/embed/iframe/CDN/Blobworker/fakeworkerなし。canvas1ページ、400万pixel、20秒budget、workerterminate。globalQueryClientで1activeviewer。外部assetsource/XFA/annotation/フォーム/リンク/scripting/fontface/systemfont/wasmなし。
- route/原本切替/非表示/正規read失効/close/unmountでabort、text/canvas/worker破棄。旧応答が新表示を閉じないepoch所有確認。PDFページ連打を同期refで封鎖。fallbackdownloadもcurrent Doc/manifest snapshotとAbortSignalで保護し、失権後bytesからObjectURLを作らない。表示とdownloadのerrorを分離する。
- 詳細閲覧既読を変更しない。表示成功を読了・同意・原本完全描画・hardheap sandboxと扱わない。PDF画像省略・非埋込font等の制約とdownload案内を日本語UI/手順に記載。

## 検証の記録

固定Node24.21.0。

- boundedtransport: 偽Content-Lengthと宣言超過のMissing expected rejection RED →上限追加後binarytransport13成功。client全体16成功（追加の受信中abort/reader解放を含む）。
- GUI: 未実装viewerのbutton欠落RED。DOCTYP文字列非実行、decode、pixel、lateclose、hidden、denial、singleowner、PDF budget/workercleanup。独立レビューで旧応答Aが新表示BをdisposeするNO-GO→再現RED→epochfixGREEN。same-tickページ連打3renders/期待2 RED→pageBusyfixGREEN。PDF失敗後fallbackdownloadがerrorを消すRED→downloadError分離GREEN。
- 最新targeted19件/3suites成功。独立最終review GO（別agent fresh19件成功、全担当filesをread-only確認）。途中全体1769成功、その後1774成功。さらに新しいerror回帰REDを全体実行中に追加したため、その中間全体は1774成功/1失敗（PDF fallbackerror testcase）となった。最終修正sourceの全体を取り直す。この途中失敗を隠さず、古い全体成功を新sourceの資格へ転用しない。
- 型チェック成功。productionbuild成功（既存webpack性能warning3件）。最終sourcebuild/型も成功。
- 実ChromiumPDF: 1件成功、CSPそのままselfworker＋nonwhite canvas、外部request0/CSPviolation0/viewerreadmutation0、keyboardEnterでopen/closeとfocus復帰、reload後自動原本取得なし。ユーザーの8080を保持し、preview原本のPORTだけ18183へ一時コピーして実施。一時filesは削除済み。実HTTP/backendAuditの試験ではなく、APIを合成responseにしたGUI受入。
- OSV2.5.1新lockscan exit0 / identified15packages / results0。全repository/Cargo資格ではない。新PDFoptionalnativecanvasにlifecycleinstallscriptなし。Browser実PDFにnativecanvasは使用しない。必須security CIはこれから。

## 未実施と次のexact action

対象PC・Tauri customscheme/WebView・実認証/TLS・実Identity/directory・実DBAudit・本番原本・hardparserheap制限・golden画像の資格・hostedCI・main統合は未実施。PDF parserheap全体の上限は保証しない。history原本viewerは追加せず、既存historydownloadを保持。

次: 最終GUI全体/型/build結果をここに追補→最終独立GO→ローカルcommit exactheadを親へ報告。push/PRは親の全branchreviewと統合順序判断後。同一機能Draftへ操作/依存/検証をまとめ、mainmergeはhead一致・必須CI・独立review後に親が判断する。

## 最終ローカル資格

2026-10-08。main6a34de3基点の最終製品差分で、GUI77 suites1775件/0失敗/0skip、targeted19件、client16件、型/build、diff検査成功。Chromium実PDF/keyboard/selfworker/CSP受入も最終buildで1件成功。独立最終レビューGO。直前の全体1774成功/1失敗は修正前のerror regressionとして保持し、この最終結果で修正を確認した。一時portfilesは削除済み。ローカルcommit headは親へgit出力で報告する（本書へ自身のhashを埋める循環は作らない）。

次のexact action: 親がこのローカルcommit/treeを全branchreview→main現在状態との結合を確認→同機能Draft PRへpush→exact-head mandatoryCI→独立review/headguardmerge→統合後mainCI。Tauri/対象PC/実DBAudit/golden/本番Identityの未実施枠は残す。

## 親レビューの補完: 履歴・native受信（2026-10-08）

baseline `1af7ec32abc8f1f27568154cd477b3e3d4966f95` を保持し、次の別commitで補完する。承認済みviewer範囲として、履歴選択原本を既存downloadTarget guardへ接続。手動historydownloadと詳細表示時の既読仕様は保持。normal/historyのbinary拒否は成功cache/表示を破棄し、新contextへ旧拒否を適用しない。

Desktop proxyは従来257MiBをJS Response前にbufferしていたため、viewerのGETにローカル専用下限headerを追加。正整数1件/min(global)/backend非転送/不正は接続前400、Content-Length/実chunkを制限。既存MIME無害化を維持し、octet-streamのPDFは選択manifest＋%PDF-署名で判定。Web GUIは同originで、別origin CORS構成は未資格。

RED: nativeoctet-streamのPDF parser呼出0、history guard未接続、native小上限でも旧proxy200、bounded GET header未送信を確認してから修正。GREEN: actual proxy.rs直接include Rust harness26件、client17件、component15件、履歴表示/拒否/旧応答試験、実Chromium PDF2件（PDF MIME/汎用MIME）成功。Rustfmt desktop/harness、型チェック、productionbuild（既存warning3）、diff検査成功。GUI77 suites1781件成功後、旧403/newcontextとhistory遅延の追補を最終再実行して結果を記載する。

ログ: `/tmp/viewer-proxy-final.log`, `/tmp/viewer-client-incremental-final.log`, `/tmp/viewer-incremental-e2e.log`, `/tmp/viewer-incremental-build.log`, `/tmp/viewer-incremental-type.log`, `/tmp/viewer-final-all-gui.log`。Rust harnessは同PRに含む独立workspaceでtargetはignore。Tauri/WebView/native配備/実backendAuditは未検証、harness合格をフルTauri合格と扱わない。PDF全heap/一時chunk/Veccapacityの厳密上限は保証しない。CI追加は親とCloud所有者が調整し、こちらではCI定義を変更しない。

最終追補sourceのGUI全体は77 suites1783件/0失敗/0skip（84.083秒）、component15件、history51件、client17件、actualproxyRust26件、実PDFChromium2件成功。最後の型チェックもexit0。historyのqueryFn未指定に関する既存console診断は出るが、試験は全件成功。finalsource logは上記/tmp。CI案は既存mandatory `desktop-runtime-bridge` 内、bridge実行前にharness `cargo test --locked` を追加する構成を親へ伝えた。CI変更・push・PR作成は行っていない。

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

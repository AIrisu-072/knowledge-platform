# 原本ビューアのPDF依存と実行境界

2026-10-08。ユーザー承認text+PDF viewerの実装判断。CSP拡張・本番アクセス変更は含まない。

`pdfjs-dist@6.4.299`を完全固定する。公式release2026-10-03、npm package version/license/integrityと取得packageを照合。Apache-2.0 LICENSEをapps/document-web/third-party-noticesへ保存する。

- https://github.com/mozilla/pdf.js/releases/tag/v6.4.299
- https://raw.githubusercontent.com/mozilla/pdf.js/v6.4.299/src/display/api.js
- https://github.com/mozilla/pdf.js/security/advisories/GHSA-wgrm-67xf-hhpq
- https://registry.npmjs.org/pdfjs-dist/6.4.299
- npm SHA512: `sha512-AVl138zALtfaAPvADulE0PZThbYzCBS79nL4pOSL/6Sm/4AH5A21BD9VHt97OlCuzJuCpmeZtAtkinisF4Vb1g==`

旧CVE-2024-4367は4.2.67で修正済み。固定6.4.299 APIにはisEvalSupportedがなく、旧optionを安全保証にしない。ユーザーのPDF scripting/annotation/form/link/XFA viewer層を使わず、canvas描画だけにする。外部URL/assetsourceを渡さず、fontface/systemfont/wasm/workerfetchは無効。workerとlibraryは同じ固定packageからselfhostする。PDFWorker.create({port})を使い、customscheme自動Blob wrapper/fake workerを回避する。

PDF.jsのoptional dependency `@napi-rs/canvas@1.0.10` と各platform packageがlockに入る。Mac arm64 optional binaryもインストールされた。main packageはMITでLICENSEを保存済み。install/preinstall/postinstall lifecycle scriptはない（build等の開発scriptは存在）。これはNode向けnative canvasであり、WebView/browser rendererはDOM canvasを使う。native packageをbrowserへimport/実行しない。OS/CPU optional packageは対応platformだけinstallされ、LinuxでMac binaryを要求しない。依存導入時にallowbuild scriptやアプリのruntime許可を追加していない。

2026-10-08ローカルOSV2.5.1 `scan source --lockfile=pnpm-lock.yaml --format=json` はexit0、identified15 packages、results0。全repositoryのCargoや既存依存をすべて確認した証拠へ読み替えない。既存RUSTSEC-2024-0436 unusedignore通知があった。mandatory security:deps CIは別途必要。

Chromium実PDF検証は既存previewのCSPそのままで同一originworker読み込み、nonwhite canvas描画、外部request0、CSP violation0、viewer操作でreadmutation0、close/reloadで自動再取得なしを確認した。ユーザー所有8080は変更せず、一時previewコピーのPORTだけ18183に変更した。一時filesは製品へ含めない。Tauri customscheme/対象PCは未実施であり、ブラウザー合格をnative資格へ転用しない。

10MiB入力/400万pixel画像・canvas/1activeviewer/1page/20秒deadlineは実用的な制限である。PDF parser全体のheapは厳密に制限しない。maxImageSizeは超過画像を省略するPDF.js仕様であり、完全な原本描画を保証しない。画面と操作手順で欠ける場合のdownload案内を出す。原本の読了・同意・署名の有効性をviewer成功から推定しない。

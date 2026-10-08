# text / PDF 原本ビューア実装計画

状態: APPROVED SCOPE / 実装中。ユーザーは2026-10-08 06:23:37 UTCに親へ「この方針で進めてください」と回答し、text/PDF原本ビューア、現行detail表示→既読契機保持を承認した。directoryはクラウド親が担当する。基点6440e026、branch feat/document-viewer-20261008。実アカウント/TLS/CSP拡張を承認した記録ではない。

## 方式

既存の認可・Audit commit後binary APIから明示クリックで取得。10 MiBをmetadataと実受信で制限し、UTF-8 textはReact text node、PDFはPDF.js固定6.4.299のcanvas-only rendererで表示する。PDF標準object/embed/iframeはTauri CSPのobject-src/frame-src noneに反するため使わない。workerはWebpackから同一originの.js module assetに出力し、明示Worker portでPDF.jsに渡す。custom schemeをBlob wrapperへ変換しない。失敗時はfake workerへfallbackしない。CSPは変更しない。

PDFデータのみを渡し、URL/font/cMap/wasm等の外部sourceを渡さない。XFAは無効、annotation/フォーム/リンク/JavaScript/scriptingを実行するviewer層を使用しない。disableFontFaceで動的fontを作らない。画像上限による省略・非埋込CJK等の表示差は画面/手順でdownload案内を示す。useSystemFonts false、useWasm false、useWorkerFetch false、stopAtErrors true。6.4 APIにはisEvalSupportedが存在せず、旧eval optionを安全策と偽称しない。1ページずつ4M pixels以内、decoded image maxImageSize4M、canvasMaxAreaInBytes16MiB、20秒deadlineでworker破棄。ページ切替は旧render cancel/cleanupから次へ。

制限の意味: 受信bytes・canvas・decoded image・同時ページ・時間を制限する。PDF.js parserのheap全体に硬い上限はなく、圧縮入力展開に対するOS memory sandboxではない。この残リスクを隠さず記録する。完全なheap上限が必要なら新しいnative/backend isolation設計が必要。大きい/未対応/暗号化/不正PDFは通常download案内を示す。

## 作業順と担当file

1. binary-transport.test.mjsで偽Content-Length/未知length/境界/abortのREDを確認。binary-transport.tsとdocument-api.tsにoptional maxBytes bounded readerを追加。upload codeを変更しない。
2. viewer純粋helperとDOMでtext非実行・古い応答/route/query失効/非表示・容量・二重click・無読込復帰のRED。new application/document-original-viewer.tsとnew DocumentOriginalViewer.tsxを実装。
3. new pdf-renderer.ts、PDF.js依存/lock/notice、Webpack worker asset。renderer契約（外部sourceなし・破棄・canvas限界）と実PDF Chromium検証を追加。
4. DocumentDetailPageのOverview原本一覧へ接続。同じ固定Version/purposeのread正常/idle/非失効を送信時と表示時に確認。既読operationを呼ばない。directory/原本書込みUIは変更しない。
5. 型・全GUI・binary client全試験・build・asset/CSP検査、日本語操作文書、独立レビュー。実native・本番対象PC・CI未実施を区別する。root承認までpush/PRしない。

## 依存根拠

- https://github.com/mozilla/pdf.js/releases/tag/v6.4.299 （2026-10-03公式release）
- https://mozilla.github.io/pdf.js/api/draft/module-pdfjsLib.html （API。固定package types/sourceと照合する）
- https://github.com/mozilla/pdf.js/security/advisories/GHSA-wgrm-67xf-hhpq （旧arbitrary JS execution修正。修正後だから全PDF安全とは扱わない）
- Apache-2.0 licenseを公式packageで保存し、transitive/optional依存・lock・security gateを確認する。

## 回復/破棄

固定Doc/Version/purpose/item/representationとmanifest snapshot/read generationを保持する。abort、非表示、unmount、read失効、route/原本切替でtext/canvasを直ちに空にしworker/renderを破棄。古いPromise結果から表示を作らない。visibility復帰や再起動で自動再取得しない。既読は詳細正常表示時の現行動作を維持し、viewerは読了証明・同意・既読新triggerを追加しない。

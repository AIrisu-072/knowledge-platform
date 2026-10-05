# 文書共通属性3項目の編集GUIの状況

## 2026-10-05 04:58 UTC — 初回hosted失敗とラベル修正

- 公開[PR71](https://github.com/AIrisu-072/knowledge-platform/pull/71) exact `8d53d7a3931b0e23f7929b85eb6846213985c71e` / tree `f728e6008ad0b892c5caaeab8d46db64f3a35e82` の[CI37264679347](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37264679347)、実runtime job `111618856328` は失敗。journeyは12件成功/1件失敗、metadata専用specだけが `gui-metadata-created` 後、初期入力値の確認で停止した。再起動以降は未実行。元の失敗証跡を保持し、旧headを受入済みとは扱わない
- 有限診断では当該ケースのAPI取得は成功し、CSP/page/console/API失敗はなかった。rawエラー本文は公開しておらず、診断不足を新しい観測基盤やログ公開で補っていない
- pin済みPlaywright 1.63.0の既存label matcherをjsdomで実行し、非空のcontrolled textareaの本文が親labelの文字列へ含まれ、exact名が一致しないことをRED2件で再現。React Testing Libraryのlabel照合だけでは検出できていなかった
- 製品修正は3項目と理由textareaへ、見えている文言と一致する明示 `aria-label` を付ける2行だけ。既存Evidenceフォームと同じ方式で、値・PATCH契約・runtime locator/timeout/retry・診断・依存は変更しない
- 回帰は初期値あり、全4項目への入力後、結果不明で閉じる/再表示した後のexact一致を確認。focused36件、全GUI295件/23 suites、application/runtime型、schema freshness、production build、collection-only journey13件/persistence2件がPASS。Webpack advisory3件は継続。独立レビューはGUI34件と実matcherの反例を別途確認してGO（未解消Critical/Importantなし）。review対象editor blob `8baa798da21528e5e7930030a9edeac60cbe60e4`、test blob `b3a04f2124182fb151e8abe2b22f7070061e1881`

次の操作：限定修正を親へdelta packetで渡し、新しい公開headで同じ実runtimeと全CIを確認する。ローカルDB/socket/listener/browserは実行せず、修正後の実受入成功はまだ主張しない。

以下は初回候補の履歴。

---

## 2026-10-05 UTC — 独立実装・純粋検証と限定レビュー完了

- 基点：公開main `e9c7f7737f1ddac676c3880475b83fb3cd7135c7` / tree `9085ce0db85857df50ad7d80b6db71d16477df40`。branch `feat/document-metadata-editor-20261005`
- 計画：[小さい画面追加](../plans/2026-10-05-document-metadata-editor.md)。操作説明：[共通属性の編集](../../operations/document-metadata-gui.md)。凍結GUI・管理基本設計/API/backend/認可/lockは変更しない
- 実装：概要の独立フォーム、正本3項目の差分set/明示unset、理由検証、capability表示、現在認可のエラー、固定操作ID/payloadの未知結果再送。その他の属性/legacy/extensionsは保持する。概要と一覧パネルの主要表示をsnake_caseへ修正
- メモリーの寿命：既存QueryClient内で共有する未確定操作・結果のみ。未送信draftは取消/遷移で破棄し、sessionStorage等の永続draftを追加しない。異なる文書の応答を新フォームへ適用しない
- 初期試験：22失敗/3成功。そのうち21失敗は未実装UI/正本表示のRED、Homeの1件はfixtureのroot query key衝突であり、修正後に別途確認した。最小実装後は25件PASS、全GUI284件/22 suites PASS。型・schema freshness・production build PASS、既存系統のWebpack advisory3件
- 追加反例：理由のUTF-8上限/White_Space、非文字列の現値、metadata空白/改行、64KiB、capability失効、別ID応答、成功後の再読取失敗を追加。独立レビューで古い再読み込み応答が再表示フォームを閉じる問題を指摘され、成功/失敗のRED2件を確認してopening世代guardとunmount無効化で修正した
- 受入準備：既存hostedの専用spec（画像/trace/video off）へ未公開変更/公開後Minor/no-op/原本・Version・既読不変/合成human-agent一致/再起動後保持を追加。値はboolean比較、専用非添付sidecarに保持。既存の有限診断出力だけを拡張し、metadata値を外部ログや公開artifactへ送らない
- 未実行：ローカルDB/socket/listener/browser、実hosted受入、exact-head全CI。Rust/backendに変更なし。実サーバー反映は所有者が手動で行う

## 最終検証

- 純粋GUI：全293件／23 suites PASS。今回のGUI32件＋API adapter2件を含む
- 型：application/runtimeともPASS。schema freshness PASS、production build PASS。Webpackのサイズ等advisory3件は残る（main533 KiB、entrypoint547 KiB）
- 純粋runner：browser診断17件＋既存要約/visual配線12件、計29件PASS。新診断のRED1件も確認済み
- 既存API契約：15件PASS。MCP受入bundle build PASS
- collection-only：journey13件、persistence2件。収集だけで実browser/DBを動かしていない
- 独立レビュー：今回の34件を別途実行してPASS、source/pure GUI範囲GO、未解消Critical/Importantなし。application blob `954dc44080e9ac042bc7bcbc9a0038867470331b`、editor blob `4fdf15f8d0f6e1ea81545cb0b91cf14c30ee1899`、runtime spec blob `343f15cbb782c7a994fbded0ed20b01c83690ed1`
- 差分・境界：git diff --check PASS。backend、API/schema、依存/lock、migration、workflow、凍結設計原本は不変。未公開の別機能からコード等を転用していない

診断の限界：標準Playwrightのrawログ/error-contextは既存private owned一時領域へ残り得る。これは外部への公開とは区別する。今回のfixtureは固定合成データのみで、新specの画像/trace/videoはoff、追加snapshotは非添付sidecar、外へは有限の診断項目だけを出す。内部Playwright API/非公開設定/新しい検証基盤は追加していない。実行時の外部artifact公開0は、今後のexact-head hosted受入で確認する。

次の操作：正確なcommit/treeと未公開packetを親へ渡す。親の公開後にそのexact headの全CI・実DB/browser・再起動・所有cleanup・artifact公開0を確認する。GitHub公開/mergeをこの作業内では行わない。

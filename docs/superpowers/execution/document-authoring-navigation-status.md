# 編集作業への可視導線と成功通知の限定補修

## 基点・範囲

- 状態: ローカル検証・独立レビュー完了、公開後資格待ち。PR74公開head `2e1e17f4c7d1d3b2819ac57771774282c3348d0b` から独立branch `fix/document-authoring-navigation-20261005` を作成。元headと凍結済みworktreeは変更しない
- 既存[WORKING追補](../specs/2026-10-05-document-working-version-editor-amendment.md)と[小計画](../plans/2026-10-05-document-working-version-editor.md)、Frontend UXの状態通知・Navigation契約に従う
- 2026-10-05の最小追加指示は、通常画面操作で作成途中の文書へ戻れる可視入口と、現在の操作文脈に沿った成功通知。既存authoring read/capabilityを利用し、新backend・権限・業務条件・依存・runnerは追加しない

## 日本語小計画と実装結果

1. Organizationでもメインナビの「編集作業」を表示する。既存Router Linkでauthoring一覧へ移動し、タスクの未保存文案と戻り先を保持する
2. 作業版の成功通知だけを編集フォームまたは版一覧に限定する。公開フォームや概要には古い成功を表示しない。成功の`role=status`によるimplicit polite live通知、操作cache、pending/unknown/rejectedの表示・回復は維持する
3. DOM反例を先に実行し、可視ナビ・未保存文案往復・成功通知scope・pending/unknown保持と固定要求再送を確認する。既存hosted Document/Organizationの受入工程を可視操作へ接続する
4. 純粋GUI・型・build・既存runtime collectionと独立レビューを行い、小commitを親へ渡す。公開・exact-head CI・main統合は親、実サーバー導入は所有者が手動実施する

製品差分はAppShellのSPA Link追加とDocumentWorkingVersionEditorの成功表示条件の2行。通知locatorのfirst/filter化で不具合を隠していない。Document実受入の単一公開status期待は維持し、公開フォームに古い成功文言が無いことも検査する。Organization実受入は既存の共有文書参照工程で編集作業一覧・同じ文書のauthoring版一覧へ回り、タスクの未保存文案へ戻る。

## 検証

- baseline: 対象3 GUI suites / 148件成功。追加したDOM反例4件はナビ欠落と古い成功通知の残留でRED、製品2行修正後GREEN
- 最終全GUI403件 / 28 suites成功、schema freshness・型・production build成功。既存Webpack performance advisory3件は保持
- DocumentとOrganizationのruntime型、既存配線・有限診断・設定の純粋Node52件成功
- 既存MCP build成功。collection-onlyはDocument journey18件/9files・persistence5件/5files、Organization journey1件・persistence1件が成功
- 独立source/DOMレビューはGO、Critical/Important/Minor所見なし。変更2 DOM suites / 150件とdiff検査を独立実行成功。150件は全GUI403件に含まれ、unique成功件数に加算しない
- 初回offline installはmetadata不足、最初の型検査は新DOMの無効`exact` option、初回collectionは未buildのMCP helperにより停止した。これらを成功扱いせず、公式registry/固定lockの通常install、文字列name完全一致を保持する型修正、既存MCP buildで解消し再検証する
- ローカルDB/socket/listener/browser/Cargoは未実行。画像・trace・videoを生成せず、既存Document受入にもrecording offを指定。golden/skip/期待値の緩和は無い。hosted実受入と公開artifact0は新exact headで親が確認する

## 次のexact action

日本語の小commitを親へ渡す。並行の応答喪失受入5pathとは重ねず、親が統合後のexact SHA/treeを固定して既存hosted受入と全CIを確認する。今回のローカル成功を新exact-headの実DB/browser・全CI・visual資格へ付け替えない。

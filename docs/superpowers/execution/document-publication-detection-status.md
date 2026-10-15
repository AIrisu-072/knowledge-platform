# 文書の公開前検査：引用HTMLの誤判定補修

Status: ACTIVE（修正候補。GREEN検証待ち）

## 範囲と根拠

- 基点はmain `2a37d35cd228344f98e0194de16d5336fa786e3c`。PR85のDocument関連2ファイルだけを独立して補修する。Search全体は取り込まない。
- `text/plain` の記事本文に引用したDOCTYPE等を、現行の `looks_like_html` が本文全域から検出し、`FormatMismatch` にしている。
- 承認済みDSI設計§5.2とproduction計画Task3の形式整合性検査を維持する。HTMLを名乗る文書の従来判定、実HTMLで始まる文書の宣言不一致、raw hash/size検査を緩和しない。
- Mac側はこの範囲を取り込まず、クラウドの単独変更とする。GUI、scheduler、global active pointer、共通導入手順は変更しない。

## 検証

- 先に回帰試験を追加した。日本語本文中のDOCTYPE引用、media typeの大小文字・parameter、worker入力準備、BOM/空白/大小文字付きHTML偽装の拒否、HTML/XHTML宣言の従来挙動を確認する。
- クラウドにはRustツールチェーンがなく、過去に公式archiveの403が記録されている。別経路で導入せず、通常のhosted CIで修正前の失敗を確認してから同一Draftへ最小修正を加える。
- ローカル `git diff --check` は成功。Rustの実行・fmt・clippyの成功は未確認。過去のMac試験結果をこのtreeの合格とは扱わない。

次の操作：test-only Draftの通常CIで引用本文の `FormatMismatch` を確認し、同じDraftで補修と再検証を行う。mergeとserver反映は行わない。

## 2026-10-08：修正前の実行結果と最小修正

- Draft [PR107](https://github.com/AIrisu-072/knowledge-platform/pull/107)、test-only head `8db60b2e19336421c11b645b0fc74369ff4383e5` の[通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37730006543)でREDを確認した。
- rust-test job `113156744143` の `plain_text_quoting_markup_keeps_its_declared_format` が、期待した `FormatMismatch`（検出Html、宣言text/plain）で失敗した。試験は実際にcompile・実行されている。別途fmt jobは新しいrequest呼出しの折返し1箇所のみで失敗し、実ログどおりに補修した。その他の通常CI jobは成功（集約required-checkは先行失敗を反映）。
- 検知修正はPR85と同じ条件分岐。HTML宣言は本文全域、その他の宣言はBOM/先頭空白を除いた文書先頭のみをHTML判定対象とする。マーカー、PDF/OOXML優先順位、不一致/未対応エラー、raw bindingを保持する。
- 次の操作：同じDraftに修正を保存し、新headの通常CI・DSI PoC・sandboxを確認する。独立レビューと最終資格はこれから。本文検知の失敗再現は確認済みだが、新修正のGREENや実公開HTTPの成功はまだ宣言しない。

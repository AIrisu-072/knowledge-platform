# 文書の公開前検査：引用HTMLの誤判定補修

Status: ACTIVE（回帰試験のみ。修正前）

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

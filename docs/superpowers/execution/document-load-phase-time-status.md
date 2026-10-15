# 文書時間予測修正の実行状況

## 2026-10-15 — PR123統合済み、統合後CI待ち

- PR123はhead `6b58bd06c9d137393365d85e11273bf5de0acc89` のCI `38766162617` SUCCESS後、main `71d757b049e48594ee4964aa5c01db54f3e17fd2` へ統合済み。GitHubのMERGED状態とCIの正確なheadを確認した。
- 統合後main CI `38769861407` は今回の読み取り時点でQUEUED。headの成功を統合後CIの成功へ付け替えない。前提PR128のmain `29026cc7` の統合後CI `38765965535` はSUCCESS。
- smallからの1,000件予測128.93分は120分の上限を超えるため未達。10万件は未資格。分類変更・予算引上げ・過去receiptの流用で未達を隠さない。
- デモ起動・新しい長時間runは開始していない。次のexact actionはmain CI `38769861407` の終端確認と親への報告。実負荷の再開は親の調整後の別工程とし、以下の「PR123統合準備」を現在の再開手順にしない。

以下は統合前の履歴として保持する。

Status: ACTIVE / PR123 統合準備

- 承認: 2026-10-14「その対応で進めてください」。2026-10-15は非検索PRの整理・適格なPRのmain統合を優先。
- branch: `fix/document-load-phase-time-20261014`、PR #123。基点main `373941fbfbcdd2cb85ecae0d5b9530ae85160241`。
- [計画](../plans/2026-10-14-document-load-phase-time.md)、[算式・操作](../../operations/document-load-phase-time.md)。
- 実装済み: 最大64の非重複連続区間、restartだけ固定、その他は保守比例。係数2、実deadline、総80時間/cleanup15分、RSS4GiB/空き予備4GiB/disk予備1GiBは維持。
- 保存head777e4604/tree3fe5ccbcの独立レビューGO。Ubuntu全load482中481pass/実Docker専用1skip/0fail、runtime191pass、typecheck、通常run-qUY560とsmall run-N0DWfN成功/cleanup済み。
- small elapsed8626.429626ms/fixed892.484170ms、1000予測128.93分は120分超。未達を維持。10万件未資格。
- 前head CI38494330119は既存PoC unsafe-libyaml検出で未合格。修正PR124はmain ead6830eへ統合、統合後CI38502201323全14成功。
- 2026-10-15: /tmp checkout消失を確認。元branchとworkspace退避文書を保持し永続専用checkoutへ復元。main ead6830eを競合なく取り込み、固定Node24.21.0の関連26試験成功。最初のdefault Node26試験はMac/Linux RSS条件で失敗し、資格として扱わない。
- 2026-10-15: PR128は独立review GO・head b794b3eeの全14CI成功後にmain 29026cc7486070410eb0b892e63c43eb7d1f5de5へ統合。競合なく追従した。
- 次: 組合せ独立レビューと必要試験、新head全CI、期待head指定merge、統合後CIを確認する。同じPR123で完結する。PR128統合後CIも並行確認する。
- デモ起動と新しい長時間負荷試験は今回開始しない。親へ統合結果を報告し、必要な調整後の別工程とする。再試行・予算引上げ・固定分類拡張で未達を隠さない。

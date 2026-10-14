# Search PoCの未使用analysis依存整理

Status: ACTIVE / 旧実装の基準取得とTDD中

2026-10-14の「CI成功後に統合していいです。」を確認。PR123の時間予測修正と別に、最新main373941fbからbranch `fix/search-poc-segmenter-dependencies-20261014` を作成した。Search本体やSearch作業PR85/86/99を統合しない。

PR123 CI38494330119は12成功/security+required-check失敗。mainとPR123のSearch PoC lockは同一で、既存lindera-analysis→serde_yaml_ng→unsafe-libyaml0.2.11のRUSTSEC-2024-0449に起因する保守。security例外・scan除外は追加しない。

承認範囲はPoCのみのTokenizer→同版Segmenter直接呼出し、不要transitive lock削除、報告候補8→7、同等性試験と実measure-lexical。lindera/IPADIC6.2.0/Tantivy0.26.2、辞書、既存fixtures、Search本体を保持する。

担当境界: modelはCargo.toml/lexical.rs/report.rs/試験、operationsは専用Ubuntuで旧golden・RED/GREEN・実測、親は最小lock差分/測定報告/独立レビュー/単一日本語Draft/CI/統合。

次のexact action: 旧実装全fixture title/summary/alias/queryと空/英数/記号/混在のtoken surface/order/offsetを実取得。新helper欠落と候補数旧8のRED確認後のみ実装。旧新lexical全caseIDs/quality/missing/indexbytes比較、PoC test/verify/denyとroot再帰OSV、exactheadレビューと全CI。互換差があれば停止報告する。

追記: Mac固定Rustで旧mainから基準59入力/257tokenと両13ケースを取得。RED→GREENで実装し、token/offset/ID/品質bits/indexbytes完全一致、旧新各10評価の安定値一致、新measure-lexical実測・lexical timings更新。cargo-deny/原本root再帰OSV成功（既存除外維持、追加なし）、verify/fmt成功。全PoCは27pass/1Docker環境fail。UbuntuSSHは接続timeout、実DB試験未実施がblocker。[証拠と測定](search-poc-segmenter-measurement-20261014.md)。次は独立レビュー・最終保存・単一Draft公開と全CI。実DB未資格を合格にしない。

この保守の全CI成功後Ready/期待head指定main統合/postCIを確認。そのmainをPR123へ安全に取り込み、組合せ再レビュー・必要試験・新head全CI/postCIまで確認。長pipeline起動前に親へ統合結果を返し、未承認デモとの同ホスト時間調整をする。時間予測128.9分>120分未達と予算/分類を保持。デモ起動は未承認・未実施。

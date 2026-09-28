# Rank fusion PoC evidence

2026-09-29 の B5。`cases.json` は synthetic query 5 件で、lexical / graph / vector-like の raw score 尺度を意図的に不一致にした。各 case は lexical query ID と hard-eligible resource ID を持ち、kind/audience の条件に一致する候補だけを rank 評価へ渡す。vector-like list はスコア融合の検査用入力であり、Vector/Embedding backend を採用した意味ではない。

`cargo test --locked --manifest-path experiments/search-discovery-poc/Cargo.toml --test fusion -- --nocapture` で、単一 retriever、retriever 優先順の連結、rank-only Reciprocal Rank Fusion (`k=20`) を比較した。各 case/strategy を 100 回繰り返したローカル 1 回の参考値。

| Strategy | Recall@10 | MRR | nDCG@10 | Fusion p50 ms |
| --- | ---: | ---: | ---: | ---: |
| Lexical only | 1.0 | 1.0 | 1.0 | 0.0006 |
| Graph only | 1.0 | 1.0 | 1.0 | 0.0006 |
| Priority concat | 1.0 | 1.0 | 1.0 | 0.0009 |
| Lexical + graph RRF `k=20` | 1.0 | 1.0 | 1.0 | 0.0015 |
| Vector-like only (diagnostic) | 1.0 | 1.0 | 1.0 | 0.0006 |
| Three-list RRF `k=20` (diagnostic) | 1.0 | 1.0 | 1.0 | 0.0019 |

RRF は retriever ごとの順位だけを足し、raw backend score は trace metadata に保存する。raw score の尺度を変更しても順位が変わらないことをテストした。Hard applicability は Fusion の外で適用する。適格性を適用すると5件とも適格候補が1件となるため、順位品質の差を測れる fixture ではない。Raw 候補で見えた RRF の MRR 優位は選定根拠から除外した。別の focused test は、lexical list が空でも priority concat が適格な graph hit を残すことを確認する。`k=20`、retriever の採用、production 品質や SLO は確定しない。

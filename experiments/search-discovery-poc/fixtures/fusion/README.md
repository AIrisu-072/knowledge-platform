# Rank fusion PoC evidence

2026-09-29 の B5。`cases.json` は synthetic query 5 件で、lexical / graph / vector-like の raw score 尺度を意図的に不一致にした。vector-like list はスコア融合の検査用入力であり、Vector/Embedding backend を採用した意味ではない。

`cargo test --locked --manifest-path experiments/search-discovery-poc/Cargo.toml --test fusion -- --nocapture` で、first-retriever only、retriever 優先順の連結、rank-only Reciprocal Rank Fusion (`k=20`) を比較した。各 case/strategy を 100 回繰り返したローカル 1 回の参考値。

| Strategy | Recall@10 | MRR | nDCG@10 | Fusion p50 ms |
| --- | ---: | ---: | ---: | ---: |
| First retriever only | 0.8 | 0.5 | 0.5786 | 0.0010 |
| Priority concat | 1.0 | 0.6 | 0.7047 | 0.0014 |
| RRF `k=20` | 1.0 | 1.0 | 1.0 | 0.0028 |

RRF は retriever ごとの順位だけを足し、raw backend score は trace metadata に保存する。raw score の尺度を変更しても順位が変わらないことをテストした。Hard applicability は Fusion の外で適用する。今回の 5 件は RRF の実装可能性を示すだけで、`k=20`、retriever の採用、production 品質や SLO は確定しない。

# Rank fusion PoC evidence

2026-09-29 の B5追加評価。`cases.json` は8件の合成query/candidate-list条件を持つ。最初の5件はkind/audienceによるhard eligibility後に適格候補が各1件となる。追加した3件は、別々の融資・預金queryで複数の適格workflowが残る2件と、graphだけが期待resourceを回収する1件である。期待resourceは対応するlexical queryの正解IDから定め、retriever順位から決めていない。

| Added case | Hard-eligible target rank: lexical / graph / priority / lexical+graph RRF `k=20` | Basis |
| --- | --- | --- |
| `eligible-graph-rescues-lexical-rank` | 3 / 1 / 3 / 1 | `ambiguous-loan` の法人融資workflow。預金・住所変更も型とaudienceは一致するがqueryの正解ではない。 |
| `eligible-noisy-graph-harms-rrf` | 1 / 3 / 1 / 2 | `ambiguous-deposit` の法人預金workflow。ノイズのあるgraph順位がRRFを悪化させる。 |
| `graph-only-corporate-loan` | absent / 1 / 1 / 1 | `compound-corporate-loan` はTantivy標準の実測で期待IDを取得できない。`graph-relations.json` の型付き三者関係を `company-a` から `product-b` 制約付きで辿ると `loan-corp` だけが得られ、testがrelation IDと候補listの一致を確認する。 |

複数適格候補のretriever順位は意図的に異なる合成入力であり、end-to-end検索品質や実運用のgraph順位を測ったものではない。Graph-onlyケースも合成関係上のsemantic oracleであり、durable Graph backendやsource routingの評価ではない。Vector-like listはraw score尺度の不一致を検査する診断入力で、Vector/Embedding backendを採用した意味ではない。Hard eligibilityはFusion前に適用し、raw scoreはtraceのみに保持する。

`cargo test --locked --manifest-path experiments/search-discovery-poc/Cargo.toml --test fusion -- --nocapture` で8件を測った。各case/strategyのrank処理を100回反復し、ローカル5 runのp50中央値を示す。sub-microsecondの数値は測定雑音の影響が大きく、production SLOやstrategy間の速度差を主張しない。

| Strategy | Recall@10 | MRR | nDCG@10 | Fusion p50 ms |
| --- | ---: | ---: | ---: | ---: |
| Lexical only | 0.875 | 0.7917 | 0.8125 | 0.00025 |
| Graph only | 1.0 | 0.9167 | 0.9375 | 0.00025 |
| Priority concat | 1.0 | 0.9167 | 0.9375 | 0.000375 |
| Lexical + graph RRF `k=20` | 1.0 | 0.9375 | 0.9539 | 0.000625 |
| Vector-like only (diagnostic) | 0.875 | 0.7500 | 0.7827 | 0.00025 |
| Three-list RRF `k=20` (diagnostic) | 1.0 | 0.8750 | 0.9077 | 0.000792 |

Lexical+graph RRFのMRR差 `+0.0208` は、順位が改善する合成caseと悪化する合成caseを含む8件での値に限る。特に最初の5件には順位を弁別する適格候補が1件しかない。`k=20`、retrieverの組合せ、production品質は確定していない。Priority concatはlexical missからgraph hitを保持できるが、改善caseで期待resourceを3位に置く。どちらもS1の最終選定には追加の判断が必要である。

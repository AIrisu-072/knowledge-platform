# Japanese lexical PoC fixture and measurement

2026-09-29 の B2 測定。13 query / 12 resource は全て架空の一般的な業務語で、顧客データを含まない。

- `resources.json` SHA-256: `968884c9befa1ad61805d66a12bfcc9067794e2af26bc2087a8767a703f552cd`
- `queries.json` SHA-256: `1e386096e607bfc04a7e19cb90dfd5c99eb3a343314d9ee487f8e42a74d7f030`
- 再実行: `cargo run --locked --manifest-path experiments/search-discovery-poc/Cargo.toml --bin search-discovery-poc -- measure-lexical --format json`

Tantivy 0.26.2 の default tokenizer と、Lindera 6.2.0 + IPADIC の形態素分割を Tantivy 0.26.2 に渡す候補を比較した。後者は `lindera-tantivy` adapter ではない。各候補を交互に 5 回評価し、各回で 13 query を 5 回ずつ検索した。下表は 5 回の中央値。unoptimized local build の参考値であり、production SLO ではない。Query latency は前処理、parse、top-10 search、document fetch を含む。

| Candidate | Recall@10 | MRR | nDCG@10 | Index bytes | Build ms | Query p50 ms | Query p95 ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Tantivy default | 0.8462 | 0.8077 | 0.8178 | 5,035 | 96.16 | 0.072 | 0.159 |
| Lindera IPADIC pretokenization | 0.9231 | 0.8231 | 0.8475 | 5,939 | 100.18 | 0.150 | 0.196 |

必須 exact/alias 11 case は両候補が期待 resource を top-10 で回収した。法人/個人、規程/手順などの紛らわしい hit は候補生成後の hard discriminator で除外できた。追加 diagnostic case の `compound-corporate-loan` は両候補で未回収、`compound-location-change` は Tantivy default のみ未回収だった。

評価値は raw lexical top-10 に対するもの。Index bytes は Tantivy の生成ファイルだけを数え、組込み辞書や実行バイナリを含まない。`cargo deny` は crate metadata を確認できたが、build 時に取得する辞書データそのものの利用条件は未確認。13 case だけで Lindera の production 採用、辞書 packaging、性能要件は決めない。B6 の選択記録を参照。

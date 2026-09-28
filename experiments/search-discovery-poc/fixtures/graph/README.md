# Typed HyperEdge backend PoC evidence

2026-09-29 の B3/B4。`relations.json` は架空の relation 9 件で、同じ borrower を持つ別 relation、role swap、evidence namespace、high-degree concept を含む。`generate.rs` は sparse/moderate/high の graph を決定的に生成する。

`cargo test --locked --manifest-path experiments/search-discovery-poc/Cargo.toml --test graph_backend -- --nocapture` で、disposable PostgreSQL 18.6 incidence table と pure-Rust reference を比較した。one-relation、constrained multi-step、3 つの生成 profile で relation ID と到達 resource が一致し、両候補が制約なし高次数展開を budget error にした。別 relation を辿る 2-hop path は relation ID を 2 個保持し、1 個の relation に合成しない。

以下はこのローカル実行 1 回の参考値。Query p50 は各 25 回、デバッグ情報を削った unoptimized build。PostgreSQL の container 起動と接続は load time から除外した。

| Profile / case | Relations | Participant rows | Rust build ms | Rust heap lower bound | PG load ms | PG table/index bytes | Rust query p50 ms | PG query p50 ms | Returned rows / distinct relations / expanded nodes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| One relation | 9 | — | — | — | — | — | 0.0012 | 0.444 | 1 / 1 / 1 |
| Constrained 2-hop | 9 | — | — | — | — | — | 0.0038 | 0.864 | 3 / 2 / 3 |
| Sparse | 128 | 256 | 0.434 | 69,559 B | 7.846 | 172,032 B | 0.0013 | 0.668 | 1 / 1 / 1 |
| Moderate | 512 | 1,024 | 1.692 | 225,217 B | 13.864 | 417,792 B | 0.0049 | 0.765 | 8 / 8 / 8 |
| High degree, constrained | 1,024 | 2,048 | 3.544 | 449,405 B | 23.455 | 679,936 B | 0.0375 | 1.710 | 1 / 1 / 1 |

Rust heap 値は構造体、文字列、Vec capacity の下限見積りで、BTreeMap node と allocator overhead を含まない。PG bytes は 2 table と index の size で、WAL、container、server memory を含まない。Returned rows は PoC SQL の結果行であり、PostgreSQL planner が走査した行数ではない。

Rust reference は process-local で、restart 後に再構築が必要。PostgreSQL は durable backend 候補だが、PoC は毎回 table を drop/reload しており、障害復旧、generation readiness、concurrent update、access/temporal/authority filter を測っていない。現時点で durable backend を選定しない。

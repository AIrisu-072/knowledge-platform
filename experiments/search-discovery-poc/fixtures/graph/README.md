# Typed HyperEdge backend PoC evidence

2026-09-29 の B3/B4。`relations.json` は架空の relation 9 件で、同じ borrower を持つ別 relation、role swap、evidence namespace、high-degree concept を含む。`generate.rs` は borrower / product / collateral / branch の4 role を持つ sparse/moderate/high graph を決定的に生成する。

`cargo test --locked --manifest-path experiments/search-discovery-poc/Cargo.toml --test graph_backend -- --nocapture` で、disposable PostgreSQL 18.6 incidence table と pure-Rust reference を比較した。one-relation、constrained multi-step、3 つの生成 profile で full path evidence が一致し、両候補が高次数展開を branching/path budget error にした。両 backend は同一の relation 入力検証を通し、重複 participant や空 ID を拒否する。別 relation を辿る 2-hop path は relation ID を 2 個保持し、1 個の relation に合成しない。

以下はこのローカル実行 1 回の参考値。Query p50 は各 25 回、デバッグ情報を削った unoptimized build。PostgreSQL の container 起動と接続は load time から除外した。

| Profile / case | Relations | Participant rows | Rust build ms | Rust heap lower bound | PG load ms | PG table/index bytes | Rust query p50 ms | PG query p50 ms | Returned rows / distinct relations / expanded nodes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| One relation | 9 | — | — | — | — | — | 0.0010 | 0.484 | 1 / 1 / 1 |
| Constrained 2-hop | 9 | — | — | — | — | — | 0.0029 | 0.872 | 3 / 2 / 2 |
| Sparse | 128 | 512 | 0.599 | 112,675 B | 17.980 | 221,184 B | 0.0009 | 0.634 | 1 / 1 / 1 |
| Moderate | 512 | 2,048 | 2.421 | 396,269 B | 26.107 | 606,208 B | 0.0041 | 0.741 | 8 / 8 / 1 |
| High degree, constrained | 1,024 | 4,096 | 4.923 | 790,219 B | 40.520 | 1,081,344 B | 0.0344 | 1.595 | 1 / 1 / 1 |

Rust heap 値は構造体、文字列、Vec capacity の下限見積りで、BTreeMap node と allocator overhead を含まない。PG bytes は 2 table と index の size で、WAL、container、server memory を含まない。Returned rows は PoC SQL の結果行であり、PostgreSQL planner が走査した行数ではない。Expanded nodes は各 step の frontier path から SQL を発行した回数で、同じ resource に到達した複数 path は別々に数える。

Rust reference は process-local で、restart 後に再構築が必要。PostgreSQL は durable backend 候補だが、PoC は毎回 table を drop/reload しており、障害復旧、generation readiness、concurrent update、access/temporal/authority filter を測っていない。現時点で durable backend を選定しない。

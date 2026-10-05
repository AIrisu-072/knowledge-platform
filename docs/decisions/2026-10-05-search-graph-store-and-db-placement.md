# Searchの永続Graph保存先とDB配置の判断

## 判断（所有者、2026-10-05）

1. **永続Graph（P3）はPostgreSQLで実装を進める。これは暫定採用であり、本番前にGraph保存先を選定し直す。**
2. **DBは当面Documentと同じPostgreSQLデータベースに同居させ、将来Searchを別サーバへ分けられる形で作る。** 分離の要否は運用状況で決める。

## 経緯と根拠

- 仕様の選定台帳では、Graph保存先は `POC REQUIRED`（計測後に選定）のままだった。PostgreSQLに決めた記録はなかった。
- 計測（[P3計測ワークフロー](../../.github/workflows/search-p3-qualification.yml)、run `37255132277`、3候補×100/1000/3000）では、再起動・別環境への復元・故障注入は3候補ともPASSした。p50はredbが最速、PostgreSQLが約2倍、Neo4jが最も遅かった。
  - p50（1 reader）：redb 約9〜18ms、PostgreSQL 約22〜24ms、Neo4j 約36〜39ms。
  - p95（8 readers）：redb 約122〜141ms、PostgreSQL 約250〜295ms、Neo4j 約309〜319ms。
- 公開ゲートについて、[事前固定した計測手順](../../experiments/search-graph-poc/qualification-method.md)は「PostgreSQLのSource行と原子的に公開できること」を前提にしていた。このためredbとNeo4jは、手順上PASSになり得なかった。同じプログラムの規則で、PG以外の公開手順の採用は所有者判断が必要なHard Stopとされている。
- 本判断はPostgreSQLを「本番の最終選定」とはしない。P3-P04の選定は暫定採用として扱い、本番前に再選定するゲートを残す。

## 再選定に備えて守ること

- Graphは `HyperGraphRetrieverPort` と、P7の `DurableGraphGenerationPort` の裏に置く。PostgreSQL固有の型・SQLを、application層やDiscoveryの意味へ漏らさない。
- Graphの表は、凍結設計どおり別スキーマ・別移行台帳に置く。
- 再選定の候補と観点は、計測した3候補（PostgreSQL、redb、Neo4j）に限らない。運用規模、遅延目標、分離後の配置、バックアップ・復元、ライセンスを本番前に改めて評価する。

## 分けられる形で作るために守ること

- Searchの表は、Search独自の移行台帳 `search_runtime_sqlx_migrations` と `search_` 名前空間に置く。Graphは別スキーマに置く。
- Searchの表からDocumentの表へ外部キーを張らない。2026-10-05時点のSearch移行 `0001`〜`0003` の外部キーは、すべてSearchの表同士である。
- SearchのコードがDocumentのデータを読むのは、Source adapter（`search-source-document`）とoutboxを経由する場合だけとする。
- 現在の唯一の結合点は、凍結済みP6/P7の「outbox取り込みとSearch世代切り替えを同一トランザクションで確定する」処理である。別サーバへ分ける時は、次の手順で置き換える。
  1. outbox中継と、重複・順不同に強い取り込みへ置き換える。Searchは取り込み時にSourceを読み直すので、意味は変わらない。
  2. Searchのスキーマを新しいDBへ移す。
  3. 字句索引などの外部成果物は、再構築するか検証付きで複製する。

## 範囲外

本判断は、本番採用、本番DB移行、mainへのmerge、deployを意味しない。凍結済みP6/P7の同一トランザクション設計は変更しない。

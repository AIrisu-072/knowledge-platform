# Search Platform本番化プログラム — 状態

## 2026-10-06

**ACTIVE / WIP。** [計画](../programs/search-platform-production/plan.md)。基点はmain `ce8ed4f`、branchは `feat/search-platform-production-20261006`。

完了：A1。判定手段（selector）が無いSourceの候補や、別の主語についての主張は、Claimの判定に含めない。判定結果が一件も無い必須Claimは、候補があれば `unknown` として結果に残し、範囲内に判定できるSourceが無ければ `no_evaluating_source` の理由（公開コード `UNSUPPORTED_COVERAGE`）を付ける。`search-application` の全379件と、実DB/FS・実TCPの `server_e2e` 7件が成功した。複数SourceのDiscoverは `sufficient` を返す。

完了：B4。`search-runtime::durable_read` の `DurableDocumentReadModel` は、Sourceの現在ポインターが変わると、P7-12と同じ再検証（保存データとcomposite digest、封印した字句検索ディレクトリ、Graph行、ポインターの二つのdigest）に合格した世代だけを新しいメモリ上の読取りモデルへ載せる。世代ごとに独立したstoreと字句検索の索引を作り、要求が手放せば破棄される。`DurableDocumentPorts` が本番用の `ActorPortsFactory` で、Documentのactor権限とResource読取りはhostの `DocumentActorAccessPort` から受け取る。実DB/FSの `durable_api` 試験で、4 route、ポインター移動後の新世代、再検証に失敗した世代の拒否（503）を確認した。Graph（hypergraph）の読取りはB7で接続する。

完了：B6。migration `0006_search_incremental_bundle_v1.sql` で、差分（INCREMENTAL）世代を自己完結のP7バンドルにした。子行（保存データ・受領記録・字句検索）は生きているGraph構築guardの下でだけ書け、READYはそのguardが生きていてコピー検証済みのときだけ許す。同じmigrationで、二つのguard関数のsearch_pathに `pg_temp` を末尾で明示し、カタログ関数をすべて修飾した（FULLの規則は不変）。現在のREADY世代があるとき、`PgDocumentIndexRuntime` は差分世代として登録し、Graphは基準からコピーして閉包を証明した差分を一回で適用、保存データと字句検索ディレクトリは新しいバンドル全体を書く。手動は `publish_incremental_manual`、fenced outboxは既存の完了ポートが世代行の種別からGraph構築guardを選び、ポインターCAS・受領記録・guard削除を一つのコミットで行う。登録後に失敗した差分は中止し、次の構築を全件にする。`incremental_bundle` の2件（手動と配送）と `search-runtime` 全125件が成功した。

次の作業：B5（ホスト登録一覧の公開）。

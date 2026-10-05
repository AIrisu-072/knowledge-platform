# Search Platform本番化プログラム — 状態

## 2026-10-06

**ACTIVE / WIP。** [計画](../programs/search-platform-production/plan.md)。基点はmain `ce8ed4f`、branchは `feat/search-platform-production-20261006`。

完了：A1。判定手段（selector）が無いSourceの候補や、別の主語についての主張は、Claimの判定に含めない。判定結果が一件も無い必須Claimは、候補があれば `unknown` として結果に残し、範囲内に判定できるSourceが無ければ `no_evaluating_source` の理由（公開コード `UNSUPPORTED_COVERAGE`）を付ける。`search-application` の全379件と、実DB/FS・実TCPの `server_e2e` 7件が成功した。複数SourceのDiscoverは `sufficient` を返す。

完了：B4。`search-runtime::durable_read` の `DurableDocumentReadModel` は、Sourceの現在ポインターが変わると、P7-12と同じ再検証（保存データとcomposite digest、封印した字句検索ディレクトリ、Graph行、ポインターの二つのdigest）に合格した世代だけを新しいメモリ上の読取りモデルへ載せる。世代ごとに独立したstoreと字句検索の索引を作り、要求が手放せば破棄される。`DurableDocumentPorts` が本番用の `ActorPortsFactory` で、Documentのactor権限とResource読取りはhostの `DocumentActorAccessPort` から受け取る。実DB/FSの `durable_api` 試験で、4 route、ポインター移動後の新世代、再検証に失敗した世代の拒否（503）を確認した。Graph（hypergraph）の読取りはB7で接続する。

完了：B6。migration `0006_search_incremental_bundle_v1.sql` で、差分（INCREMENTAL）世代を自己完結のP7バンドルにした。子行（保存データ・受領記録・字句検索）は生きているGraph構築guardの下でだけ書け、READYはそのguardが生きていてコピー検証済みのときだけ許す。同じmigrationで、二つのguard関数のsearch_pathに `pg_temp` を末尾で明示し、カタログ関数をすべて修飾した（FULLの規則は不変）。現在のREADY世代があるとき、`PgDocumentIndexRuntime` は差分世代として登録し、Graphは基準からコピーして閉包を証明した差分を一回で適用、保存データと字句検索ディレクトリは新しいバンドル全体を書く。手動は `publish_incremental_manual`、fenced outboxは既存の完了ポートが世代行の種別からGraph構築guardを選び、ポインターCAS・受領記録・guard削除を一つのコミットで行う。登録後に失敗した差分は中止し、次の構築を全件にする。`incremental_bundle` の2件（手動と配送）と `search-runtime` 全125件が成功した。

完了：B5。migration `0007_host_registration_inventory_v1.sql` と `search-runtime::host_inventory`。`PgHostInventoryPublisher`（P7-R01P）は、運用者の入力（登録0件のテナントも含むテナント一覧と、Document/Remoteのサーバー設定）を、構成ルートと同じ検証済みコンストラクターで登録に組み立て、不変の改訂行、前進だけのhead CAS、Search Audit `host.registration.changed` を一つのコミットで書く。同じ改訂番号の別内容は衝突、古い改訂は拒否、同一内容は再公開しない。コミット結果が不明なら別接続でheadを再読してから結果を返す。`PgHostRegistrationSource`（P7-R02）は本番用の `HostRegistrationSnapshotPort` で、headの改訂を読み取り専用スナップショットで読み、全登録を作り直して保存digestと照合する。`capture()` は両名前空間を同じ改訂から返す。`search_host_publisher` ロールを追加した。実PGの `host_inventory` 5件（空テナント・台帳reconcile、前進のみ・衝突・同一内容、不完全入力で無書込み、Audit失敗で全体ロールバック、改訂・head・Auditの書換え拒否）と `search-runtime` 全130件が成功した。

設計との差分（B5）：凍結設計はSearch Audit生成元の表をDomainの移行に置くとしていたが、Document Platformの移行に触れないため、Searchの移行0007に置いた。Audit保存先への配送worker（R04Aの残り）は未実装で、生成元の行は `delivered_at` が空のまま残る。

完了：B7。公開Discoverに任意の `graph`（起点Resource 1〜8件、関係の種類、from/toの役割、最大ホップ1〜3）を追加し、OAS `DiscoveryGraph` と契約試験を更新した（追加のみ）。名前空間、actorのaccess context、評価の時刻文脈、走査上限はサーバーが付け、呼び出し側は指定できない。routeは可視のDocument Sourceごとに走査計画を作る。永続の読取りモデルは、検証済みGraph行の構造上の所有者とともにGraphを読み込み（`DurableDocumentGraph`）、各要求はそのactorのDocument権限で入る。参加者の権限確認はその束縛のactorだけに効き、要求を抜けると束縛は消える。`durable_graph::graph_nary_third_participant_revocation`（三者関係 `document_current_placement` をDocumentからVersionへ辿る経路が、権限のあるactorにだけ存在し、権限の無いactorと抜けた後の束縛では存在しない）と、公開入力の受理・項目エラー・未知項目拒否の試験が成功した。

制約（B7）：Document Sourceの公開候補はDocument Versionだけで、Folder配置は文書ごとの構造節点のため、文書をまたぐ経路は存在しない。三者関係の参加者は同じDocumentの権限に属するため、第三参加者だけの取消は表現できず、actor単位の権限と束縛の解放で確認した。公開結果はGraph経路を開示しない。

A1の影響：`search-source-document::vertical_slice` の一件は、別の主語の主張を判定外にしたことで `Unresolved` から `Sufficient` に変わる（所有者判断①どおり）ため、期待値を更新した。

次の作業：E（Vectorの本番実装）。

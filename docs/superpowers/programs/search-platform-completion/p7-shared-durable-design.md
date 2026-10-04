<a id="p7-共有-durable-generation-基盤--設計案"></a>
# P7 共有永続世代基盤 — 設計案

[翻訳元の固定公開原文（commit 0ecf486719e3c9d71242e289a7564ad6d1032b3c）](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-shared-durable-design.md)

本書は意味保存の日本語訳であり、原設計の再承認や資格の追加ではありません。既存の承認ハッシュは当時の原文・証拠を指し、訳文のハッシュではありません。以下の状態・手順・次の作業は当時の記録であり、現在の実行許可ではありません。最新の状態は[実行状態の正本](../../execution/search-platform-completion-program-status.md)を参照してください。旧見出しアンカーは明示IDで維持しています。Searchイベント処理記録（Search receipt）はDB上のイベント処理記録を指し、工程の検証記録・証拠（receipt）とは区別します。

状態：**DRAFT（草案）/ 実装・バックエンド選定・実DB検証前**（2026-09-30）。本書は、P3 GraphとP6 Search配送が共用する基盤の実装入力である。P3の三候補の測定・選定、P6の全縦断の検証記録、P7の最終的な構成・HTTP・運用の完了を主張しない。規範は `spec/`、P1/P3/P6の凍結内容と各実装計画、[P7早期契約](p7-runtime-contract-draft.md)とする。抵触時は、P3の構築ガード追補と外部キー制約を守る回収順序の修正、P6改訂1、P1の本文追補とPartial肯定証拠の修正を優先する。

<a id="1-実装を先行できる境界"></a>
## 1. 実装を先行できる境界

- P6-I03が `crates/search-runtime/migrations/0001_search_source_coordination_v0.sql` の**唯一の書き込み担当**となり、物理表 `search_source_coordination` と `search_index_receipts`、独立台帳 `search_runtime_sqlx_migrations` を作る。P3の `source_control` はこの物理Source行の別名であり、二つ目のポインター・表・エポックを作らない。P7はSearch移行 `0002` 以降を追加する。Domain `0009` → Search `0001` → P7 `0002+` の順を確認し、P3 Graphの移行は別スキーマ・別台帳とする。`search-runtime::migrate(&PgPool)` がSearch連番移行の単一の入口である。
- `outbox_events`、Source行、P7のREADY・世代固定・ガード、Searchイベント処理記録は、**同一のPostgreSQLデータベース**でトランザクションを共有する。これはGraph製品の最終採用ではない。[P3隔離PoC報告](../../../../experiments/search-graph-poc/report.md)はPostgreSQL 18.6の接続関係表現（incidence）を実装候補にしたが、[独立監査](p3-graph-poc-review.md)はP3-P04の受入と本番バックエンドへの昇格を**NO-GO**とした。PGは継続評価の第一候補である。Graphは `DurableGraphGenerationPort` と、非公開のトランザクションに結び付いたGraphリポジトリーから接続する。非PG候補が選ばれた場合、Graph READY・世代固定・GCとポインターCASについて同等のクラッシュ・フェンスのプロトコルを別途凍結・審査するまで、その候補を本番公開に接続しない。共有PG調整基盤のスキーマ・ポートは、P3やP6の全統合の検証記録を待たずに進める。Graphの本番移行とREADY接続は、P3-P04の再判定後に行う。最終P7は両方の検証記録とP1/P4/P5を集約する。
- 依存方向は、`search-core` の型 → `search-application` のSQLxに依存しないポート ← `search-runtime` のアダプターとする。`search-runtime` はGraph・ストレージ、Document接続処理、汎用配送を組み立てる。`search-source-document`、`search-graph`、`outbox-delivery` から `search-runtime` へ依存させない。`PgConnection` / `sqlx::Transaction` は、ランタイム内の非公開のトランザクションに結び付いたインターフェースにだけ現れる。Graph PGアダプターを選ぶ場合も、READY確認、ポインター、世代固定、GCは**同じ接続**で行い、別プールの読み合わせを原子性と呼ばない。
- SQLxは、ワークスペースとロックファイルの `0.9.0` を用いる。同版の [`Migrator::dangerous_set_table_name`](https://docs.rs/sqlx/0.9.0/sqlx/migrate/struct.Migrator.html#method.dangerous_set_table_name) で `search_runtime_sqlx_migrations` を指定できるが、この名称は最初の適用前に固定し、適用済みの台帳を後から空の表に切り替えない。`migrate!` の埋め込みソース、独立台帳のチェックサム・順序、Domainの `_sqlx_migrations` とGraph台帳との非干渉を、実DBで検証する。第三者の `sqlx_migrator` のAPIを、SQLx本体のAPIと取り違えない。

<a id="2-物理状態と移行-0002"></a>
## 2. 物理状態と移行 `0002+`

P6-I03のSource行は、`source_id`、`fence_epoch`、`owner_token`/`lease_expires_at`、`current_generation_id`/`current_manifest_digest`/`current_bundle_digest`、`pointer_revision`、`last_published_epoch`、`build_fence_seq` を持つ。NULL許容の組は、全項目を揃えるか全項目NULLとする。カウンターは非負とし、オーバーフローを拒否する。登録済み `SourceId` は全テナントで一意とし、取得前に一意なSource行をINSERTする。現在の世代キーは `(source_id,current_generation_id)` であり、`current_generation_id` 単独では参照しない。

`0002` 以降では、以下を**既存構造への追加**として置く。表名はSearch名前空間の提案であり、`0001` の列、イベント処理記録の主キー、台帳を改名しない。`search_index_receipts` に `bundle_version` を追加し、以後の書き込みと同一エポックの照合に必須とする。既存行がある移行では、版をダイジェストから推定して自動補完しない。検証できないイベント処理記録を過去のメタデータとして隔離し、現行のREADY実体で再処理する。全子表の `(source_id,generation_id)` 外部キーは `ON DELETE RESTRICT` とし、ペイロード書き込みロールと調整担当のGCロールを分離する。

P4のプロセス内カタログでは、SourceIdの過去の所有テナントを再起動後に証明できない。`search_source_ownership` を、**全テナントとDocument/Remoteに共通する永続台帳**として、同じ調整用DBに置く。この台帳は、`source_id UUID PRIMARY KEY`、ホストが発行する長さ制限付きの不透明な `tenant_owner_key TEXT NOT NULL`、`source_kind TEXT NOT NULL`、`registration_revision BIGINT NOT NULL`、`activation_epoch BIGINT NOT NULL`、`state ACTIVE|TOMBSTONED`、許可リストに含まれる非秘密かつ版管理された `registration_dto JSONB` とそのダイジェスト、作成・更新時刻を持つ。所有テナントとSourceIdは変更・削除できず、削除済み記録を残す。別テナントが使用済みSourceIdを登録しようとした場合は、現行のACTIVE行がなくても拒否する。全テナントで同じ台帳を使い、DBごとの独立採番を本番に持ち込まない。同じテナントによる明示的な再有効化だけは、新しい有効化エポックと世代キーを要求する。

`0002` は既存Source行に、NULL許容の `tenant_owner_key`/`registration_revision`/`activation_epoch` と `registration_active BOOLEAN NOT NULL DEFAULT false` を追加する。P4のSQLxに依存しない `SourceRegistrationLedgerPort` は、`reconcile`/`is_current` を `BoxFuture` で返す。P7の実DBアダプターの `reconcile(desired)` は、**期待登録集合全体に対する一つの原子的な決定**である。`search_registration_serial` の一行を最初にロックし、既存Source行をSourceId順、所有権行をSourceId順にロックして、一つのトランザクションで比較・更新する。Remote期待登録集合にない既存Remoteは削除済み記録にするが、Document行は消さない。初回Sourceは全体共通の直列化ロックの下で、所有権行 → Source行の順に同一トランザクションでINSERTし、コミットまでは見せない。Document登録も同じ台帳と直列化ロックを使う。所有テナントは不変とし、登録改訂番号と有効化エポックは非負・単調とし、オーバーフロー時は安全側に倒して拒否する。`RegistrationActivation(u64)` がPGの `BIGINT` 上限を超える値も拒否する。登録DTO・可視性改訂番号の変更、削除、再有効化では、有効化エポックを進め、Source所有者トークンを失効させ、`fence_epoch` もオーバーフローを確認して進める。**台帳の照合・同期は、現在の世代キー・マニフェスト・成果物一式と `pointer_revision` を変更しない**。旧現在世代は有効化状態の不一致により固定・読み取り・再利用を禁止し、新世代の明示的な公開CASでだけ置換する。削除済み記録化後のポインター解除と旧成果物の退役も、別の調整トランザクションで行い、現在の世代・固定・ガードを検査する。DBのロール・トリガーは、所有権行だけの差し替えや、Source行の有効状態・改訂番号だけの変更を拒否する。P6 Sourceリースの条件付きUPDATEは、有効状態や所有者・有効化状態との結び付けが欠けた行を取得しない。利用主体の読み取り・世代固定は、全体共通の直列化行や所有権行を先にロックせず、Sourceロック下で現在のエポックを確認する。登録更新と逆順のロックを作らない。`activation_epoch` は登録の有効化世代、`fence_epoch` は各Sourceリース取得の所有フェンスであり、混同しない。

移行済み `search_source_coordination` 行のテナントをSQLから推定しない。ホストで設定した永続的な所有権ポートで、全既存SourceIdと世代の所有者・種別・改訂番号を確認し、台帳とSource行を原子的に結合する。未対応、矛盾、重複、別テナントによる再利用が一件でもあれば、起動・登録を拒否する。適用後は、`(source_id,tenant_owner_key)` の一致と台帳の存在を、外部キー・トリガーと起動時の走査で検証する。本番にこのポートがなければ、安全側に倒して拒否する。P4の `SourceRegistrationLedgerPort::is_current` は、保存DTOを復元し、登録全体と有効化状態を照合する。プロセス内キャッシュやダイジェストだけでは、現在有効と判定しない。P4の利用主体から見える登録情報と `AuthorizedSourceScope` は、台帳のACTIVE・テナント・登録/有効化改訂番号を現在状態の確認で照合し、Graph/Projectionキーだけから所有者を推定しない。RemoteのRAM世代にも同じ全体共通の所有権ゲートを適用する。Remoteの `desired` は全テナントを含む一つの集合として渡し、テナント別カタログが他テナントの行を削除済み記録にしない。複数レプリカは同じホスト設定の配備改訂番号とRemote期待登録集合のダイジェストを使う。同じ改訂番号で異なる集合や、古い改訂番号からの再照合・同期は拒否する。`search_registration_serial` は移行・起動時に一行を初期化し、この比較をDB内で行う。

| 表 | 列・制約と所有者 |
| --- | --- |
| `search_generation_identity` | `(source_id,generation_id)` の主キー、所有テナントキー、有効化エポック、作成時刻。すべての永続構築の最初に、同じトランザクションで一度だけINSERTし、GC後も**削除しない**。同じキーの再割り当てや、別テナント・有効化世代への混入をDBで拒否する。NoRetention/SessionOnlyのRAM世代は記録しない。 |
| `search_generation` | `(source_id,generation_id)` の主キー、Source外部キー、`state ∈ {BUILDING,READY,FAILED,DELETING}`、`source_snapshot`、版管理された `projection_manifest`、`projection_manifest_digest`、`projection_resource_count`、`bundle_version`、`activation_epoch`、`ready_at`、`stage_event_id?`/`stage_source_epoch?`。P1マニフェストのSource・キー・スナップショット・スキーマ・件数・ダイジェストと行を一致させる。世代キーは再利用しない。 |
| `search_generation_payload` | `(source_id,generation_id,kind)` の主キー、`kind ∈ {projection,unit_manifest,body_coverage}`、`dto_version`、`payload JSONB`、検証済み論理ダイジェスト・件数。プロバイダーに依存しないDTOとして、全射影と意味登録情報、全 `BodyItemEntry`/`KnowledgeUnit.text`、全網羅性項目をそれぞれ保存する。未知のDTO版・フィールド、欠落、余分、重複、件数超過を拒否する。JSONBの物理バイト列のハッシュをP1の正規化ダイジェストとみなさず、復元DTOからP1のエンコーダーで再計算する。 |
| `search_generation_receipt` | キーを主キーとし、Sourceスナップショット、P1の `projection_digest`、`unit_manifest_digest/count`、`body_coverage_digest/count`、`lexical_digest/count/schema/analyzer`、`graph_input_digest/count`、`profile_set_digest`、P1の `composite_digest`、Graphのバックエンド・スキーマ・Source対応付け・内容ダイジェスト・リソース数・関係数、`vector_receipt?`、版管理された検証情報DTOを持つ。各構成要素のキーとSourceスナップショットを揃える。`composite_digest` は `current_bundle_digest` と同じ32バイト値であり、`0001` のSearchイベント処理記録の `bundle_digest` にも、同じ版を明示して書く。 |
| `search_lexical_artifact` | キーを主キーとし、信頼された索引ルート内でキーから導く相対ディレクトリ、索引形式・スキーマ、整列したファイル名＋サイズ＋SHA-256のツリーダイジェスト、実際に検索可能なResource/Unit文書の論理ダイジェスト・件数、Unit完全性照合のダイジェスト・件数、`finalized_at` を持つ。索引バイト列は、版管理された変更不能なディレクトリに置く。要求やプロバイダーから外部パスを受け付けない。Vector採用時は、同型の別成果物行・検証情報と、成果物一式の版を追加する。 |
| `search_generation_full_guard` | `(source_id,target_generation_id)` の主キー、推測不能なトークン、Source内で単調な `build_fence`、DB時計による有効期限、対象への外部キー `RESTRICT`。完全構築対象をStage→READY→CASの間保護する。増分構築は、P3の `search_graph.build_guard` が基底と対象の双方を保護し、同じ `build_fence_seq` 名前空間を使う。 |
| `search_evaluation_lease` | `(source_id,lease_id)` の主キー、サーバー発行の評価ID、世代への外部キー `RESTRICT`、マニフェストと成果物一式のダイジェスト、DB時計の `expires_at`、所有者スコープの参照。外部の世代ID・評価IDは発行権限にならない。Remote評価用RAMリースは別のライフサイクルを持ち、この表には入れない。 |

物理移行の最小SQL形式を以下に示す。長さ上限、ダイジェストの `sha256:` と小文字16進表現、版の許可リスト、変更を禁止するトリガー、ロールへの権限付与は、移行で明示する。`CHECK` だけでREADYの意味を証明したと扱わない。`current_bundle_digest` と検証情報の32バイトダイジェストは、`sha256:` + 64桁の小文字16進表現とし、デコードしたP1のバイト列と比較する。

```sql
CREATE TABLE search_source_ownership (
  source_id uuid PRIMARY KEY,
  tenant_owner_key text NOT NULL,
  source_kind text NOT NULL,
  registration_revision bigint NOT NULL CHECK (registration_revision > 0),
  activation_epoch bigint NOT NULL CHECK (activation_epoch > 0),
  state text NOT NULL CHECK (state IN ('ACTIVE','TOMBSTONED')),
  registration_dto jsonb NOT NULL,
  registration_digest text NOT NULL,
  created_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL
);
CREATE TABLE search_registration_serial (
  singleton boolean PRIMARY KEY CHECK (singleton),
  deployment_revision bigint NOT NULL CHECK (deployment_revision > 0),
  remote_desired_set_digest text NOT NULL
);
ALTER TABLE search_source_coordination
  ADD COLUMN tenant_owner_key text,
  ADD COLUMN registration_revision bigint,
  ADD COLUMN activation_epoch bigint,
  ADD COLUMN registration_active boolean NOT NULL DEFAULT false;
ALTER TABLE search_index_receipts ADD COLUMN bundle_version text;

CREATE TABLE search_generation_identity (
  source_id uuid NOT NULL REFERENCES search_source_ownership(source_id) ON DELETE RESTRICT,
  generation_id uuid NOT NULL,
  tenant_owner_key text NOT NULL,
  activation_epoch bigint NOT NULL CHECK (activation_epoch > 0),
  created_at timestamptz NOT NULL,
  PRIMARY KEY (source_id,generation_id)
);
CREATE TABLE search_generation (
  source_id uuid NOT NULL REFERENCES search_source_coordination(source_id) ON DELETE RESTRICT,
  generation_id uuid NOT NULL,
  activation_epoch bigint NOT NULL CHECK (activation_epoch > 0),
  state text NOT NULL CHECK (state IN ('BUILDING','READY','FAILED','DELETING')),
  source_snapshot text NOT NULL,
  projection_manifest jsonb NOT NULL,
  projection_manifest_digest text NOT NULL,
  projection_resource_count bigint NOT NULL CHECK (projection_resource_count >= 0),
  bundle_version text,
  stage_event_id uuid,
  stage_source_epoch bigint,
  ready_at timestamptz,
  PRIMARY KEY (source_id,generation_id),
  FOREIGN KEY (source_id,generation_id) REFERENCES search_generation_identity ON DELETE RESTRICT
);
CREATE TABLE search_generation_payload (
  source_id uuid NOT NULL, generation_id uuid NOT NULL,
  kind text NOT NULL CHECK (kind IN ('projection','unit_manifest','body_coverage')),
  dto_version text NOT NULL, payload jsonb NOT NULL,
  logical_digest text NOT NULL, logical_count bigint NOT NULL CHECK (logical_count >= 0),
  PRIMARY KEY (source_id,generation_id,kind),
  FOREIGN KEY (source_id,generation_id) REFERENCES search_generation ON DELETE RESTRICT
);
CREATE TABLE search_generation_receipt (
  source_id uuid NOT NULL, generation_id uuid NOT NULL,
  source_snapshot text NOT NULL, receipt_version text NOT NULL,
  projection_digest text NOT NULL,
  unit_manifest_digest text NOT NULL, unit_count bigint NOT NULL CHECK (unit_count >= 0),
  body_coverage_digest text NOT NULL, body_item_count bigint NOT NULL CHECK (body_item_count >= 0),
  lexical_digest text NOT NULL, lexical_count bigint NOT NULL CHECK (lexical_count >= 0),
  lexical_schema_version text NOT NULL, lexical_analyzer_version text NOT NULL,
  graph_input_digest text NOT NULL, graph_input_count bigint NOT NULL CHECK (graph_input_count >= 0),
  profile_set_digest text NOT NULL, composite_digest text NOT NULL,
  graph_backend text NOT NULL, graph_schema_version text NOT NULL,
  graph_mapping_digest text NOT NULL, graph_content_digest text NOT NULL,
  graph_resource_count bigint NOT NULL CHECK (graph_resource_count >= 0),
  graph_relation_count bigint NOT NULL CHECK (graph_relation_count >= 0),
  vector_receipt jsonb,
  receipt_dto jsonb NOT NULL,
  PRIMARY KEY (source_id,generation_id),
  FOREIGN KEY (source_id,generation_id) REFERENCES search_generation ON DELETE RESTRICT
);
CREATE TABLE search_lexical_artifact (
  source_id uuid NOT NULL, generation_id uuid NOT NULL,
  index_relpath text NOT NULL, index_format_version text NOT NULL,
  tree_digest text NOT NULL, logical_digest text NOT NULL,
  searchable_doc_count bigint NOT NULL CHECK (searchable_doc_count >= 0),
  unit_seal_digest text NOT NULL, unit_seal_count bigint NOT NULL CHECK (unit_seal_count >= 0),
  finalized_at timestamptz NOT NULL,
  PRIMARY KEY (source_id,generation_id),
  FOREIGN KEY (source_id,generation_id) REFERENCES search_generation ON DELETE RESTRICT
);
CREATE TABLE search_generation_full_guard (
  source_id uuid NOT NULL, target_generation_id uuid NOT NULL,
  guard_token uuid NOT NULL UNIQUE,
  build_fence bigint NOT NULL CHECK (build_fence > 0),
  expires_at timestamptz NOT NULL,
  PRIMARY KEY (source_id,target_generation_id), UNIQUE (source_id,build_fence),
  FOREIGN KEY (source_id,target_generation_id) REFERENCES search_generation ON DELETE RESTRICT
);
CREATE TABLE search_evaluation_lease (
  source_id uuid NOT NULL, lease_id uuid NOT NULL, evaluation_id uuid NOT NULL,
  generation_id uuid NOT NULL, activation_epoch bigint NOT NULL,
  manifest_digest text NOT NULL, bundle_digest text NOT NULL,
  expires_at timestamptz NOT NULL,
  PRIMARY KEY (source_id,lease_id),
  FOREIGN KEY (source_id,generation_id) REFERENCES search_generation ON DELETE RESTRICT
);
CREATE INDEX search_evaluation_lease_by_generation
  ON search_evaluation_lease(source_id,generation_id,expires_at);
```

`search_source_coordination` のNULL許容の現在の世代キーに、`(source_id,current_generation_id) → search_generation` の複合 `ON DELETE RESTRICT` 外部キーを追加する。移行前にNULLでないポインターの参照先欠落を検出した場合は、移行を停止し、既存行を黙ってNULLにしない。公開・世代固定・準備完了判定は、Source行と台帳がACTIVEで、テナントと有効化エポックが一致し、候補の `search_generation.activation_epoch` と恒久的な識別情報も同じ場合だけ通す。起動時には、識別情報台帳の全キーを所有権台帳と照合し、異なる所有テナント、未知の旧形式世代、キーの再割り当てを拒否する。過去の `search_index_receipts.generation_id` は**外部キーや世代固定にしない**ため、旧世代をGCした後もイベントのメタデータを保持できる。古い期限切れの評価リース行は、同じGCトランザクションで削除してから世代を消す。

`search_generation` とペイロード・検証情報・字句索引の行は、READY後に変更不能とする。全子表のINSERT/UPDATE/DELETEトリガーは親世代をロックし、通常変更はBUILDINGでだけ許可する。READY→BUILDINGへの変更とREADY内容の変更は、DBの権限付与・トリガーで拒否する。`DELETING` と子行のDELETEは、調整担当GCロールの限定トランザクションでだけ許す。P3 Graph側も、凍結§4の行単位フェンスを維持する。DB管理者によるトリガー無効化は、信頼境界外の監査対象である。

<a id="3-ready-の意味と同一-key-の証明"></a>
## 3. READYの意味と同一キーの証明

1. 完全構築は、Source行 → 恒久的な識別情報の新規INSERT → 新規 `search_generation` とGraph世代 → 完全構築ガードを、同じ短いトランザクションで登録する。増分構築は、Source行 → 対象の識別情報の新規INSERT → **基底と対象のキー順に並べた両世代** → P3ガードを、最初のコピーより前に登録する。Graph対象とP7対象を同じキー・スナップショットに結び付け、ガードなしの対象を他プロセスから見える状態にしない。識別情報の主キー衝突と `build_fence_seq` のオーバーフローでは失敗させ、別キーを発行する。Sourceスナップショットの長時間読み取り、パーサー・字句索引のファイルI/O、Graphコピーは、Source行ロックの外で行う。
2. Projection・Unit・網羅性情報は、上記ペイロードとして全内容をDBに保存する。`ProjectionGenerationManifest.digest` は、既存の射影専用v1の `sha256:` 値を維持し、本文を混ぜない。P1 Unitマニフェストは、**Unit本文を格納したDTO**から `text_sha256` を再計算して正規化ダイジェストを求める。網羅性情報は、同じ正本項目の集合からダイジェストと件数を再計算する。`Retryable` 項目、全正本項目の列挙や原データとの結び付けの不一致、プロファイル固定値の不一致があれば、READYにしない。`Completed+Partial` の検証済みUnitは肯定証拠になり得るが、網羅性の欠落を残し、不存在を示す否定的証拠の完全性には使わない。
3. 字句索引は準備用ディレクトリを作り、全ファイルとディレクトリを永続同期し、変更不能な最終パスへ原子的に名前変更して、親ディレクトリも同期する。完成した索引を新たに開き、検索可能な全文書を列挙し、P1の字句索引の論理ダイジェスト・件数・スキーマ・解析器とツリーダイジェストを確認する。P1本文追補どおり、Unitマニフェストの全Supported/Partial Unitと検索可能なUnit文書を、**双方向に一対一**で完全性照合する。Unsupported/FailedPermanentの文書数は0とする。構築側入力の検証情報だけではREADYにしない。ファイルの消失、破損、別キーからの流用は利用不可とする。
4. Graphは、P3の `recover_ready`/`validate_ready` が、型付きn項関係の行、参加者・所有者・Sourceの対応付け、時間情報、件数、Graph内容ダイジェストを、実体から再計算する。P1の `GenerationBundleReceipt.graph.digest` は、**Graphのステージング入力＋Document所有者の対応付け**のダイジェストである。一方、P3の `GraphGenerationReceipt.graph_content_digest` は、**Source ID・スキーマ・Graphリソースと関係の全体**のダイジェストであり、両者を同じ値と仮定しない。`graph_input_count` は、添付の重複除去後の一意な型付き関係数と定め、P3の `relation_count` と照合する。両方の検証情報を、一つの復元済み正規化Graphレコード集合から独立に計算する。キー、Sourceスナップショット、対応付け、スキーマ、リソース数・関係数の対応を、`GraphReceiptMappingV1` で検査する。P1 Graph入力ダイジェストの厳密なエンコーダーとテストベクトルが固定・照合されるまで、公開を拒否する。P3内容ダイジェストをP1欄へコピーしない。
5. `GenerationBundleReceipt` v1の `composite_digest` は、P1 §3のドメイン分離子・フィールド順・件数の式を、そのまま再計算する。各 `ArtifactReceipt.key` は、同じ `(SourceId,generation_id)` とする。Graph件数がP1 v1の複合ダイジェストの入力外でも、別の検証情報の検査を省かない。Vectorを本番検索に採用する場合は、Vectorの成果物・キー・件数・ダイジェスト・モデル・保持条件を含む**新しい成果物一式の版と基準検証ベクトル**を先に凍結し、v1ダイジェストを黙って拡張しない。Vector未採用ならv1を維持する。
6. `validate_ready` は対象行のロック下で、保存ペイロード、索引の実体、Graph検証情報、保持許可、Sourceスナップショット、上記の対応を再検証する。最後に `search_generation` のREADY状態と検証情報を、一つのPGトランザクションで確定する。外部索引I/OはSourceロック中に行わず、事前検証後に変更不能なディレクトリとDBとの結び付けを再確認する。READYは「公開可能な候補」であり、現在の世代ではない。ファイルを実体とする成果物はPGと原子的に保存できないため、CAS前の検証と、後述する読み取り時の安全側の拒否が保証境界となる。索引・GraphだけがREADY、またはDB検証情報だけがREADYという状態は認めない。

<a id="4-sqlx-free-scoped-port-と公開の線形化点"></a>
## 4. SQLxに依存しないスコープ付きポートと公開の線形化点

概念上のシグネチャは以下とする。Rust実装時は既存の `SourceId`/`ProjectionGenerationKey`/P4の信頼された型を再利用し、SQLx型や `PgPool` をアプリケーションに出さない。`ReadyEvidence` は事前検証結果であり、公開権限でも永続的な世代固定でもない。

```rust
enum GenerationDomain { Durable, RemoteEvaluation }
enum ScopedExecution<'a> {
    Actor(&'a AuthorizedSourceScope),          // trusted resolver が発行
    System(&'a SourceFence),                   // P6 の DB lease を再検証
}
struct VerifiedBundle { key: ProjectionGenerationKey, manifest: ProjectionGenerationManifest,
                        receipt: GenerationBundleReceipt, activation_epoch: u64 }
enum BuildHandle { Full(FullBuildHandle), Incremental(BuildGuardHandle) }
enum ReadyEvidence { Durable(VerifiedBundle), Remote(SealedRamGeneration) }
enum ScopedPin { Durable(PinnedBundleLease), Remote(EvaluationRamLease) }
trait ScopedGenerationGate: Send + Sync {
    fn check_ready<'a>(&'a self, scope: ScopedExecution<'a>, key: ProjectionGenerationKey,
                       domain: GenerationDomain) -> BoxFuture<'a, ReadyEvidence>;
    fn pin_current<'a>(&'a self, scope: ScopedExecution<'a>, evaluation: DiscoveryEvaluationId,
                       domain: GenerationDomain, ttl: BoundedTtl) -> BoxFuture<'a, ScopedPin>;
}
trait DurableGenerationCoordinator: Send + Sync {
    fn begin_full<'a>(&'a self, scope: SourceFence, manifest: PersistableGenerationManifest)
        -> BoxFuture<'a, FullBuildHandle>;
    fn begin_incremental<'a>(&'a self, scope: SourceFence, base: VerifiedBundle,
                             manifest: PersistableGenerationManifest)
        -> BoxFuture<'a, BuildGuardHandle>;
    fn publish_if_current<'a>(&'a self, scope: SourceFence, expected: CurrentGenerationSnapshot,
                              candidate: VerifiedBundle, guard: BuildHandle)
        -> BoxFuture<'a, CasOutcome>;
    fn renew_pin<'a>(&'a self, pin: &'a PinnedBundleLease, ttl: BoundedTtl)
        -> BoxFuture<'a, ()>;
    fn verify_pin_before_return<'a>(&'a self, pin: &'a PinnedBundleLease)
        -> BoxFuture<'a, ()>;
    fn release_pin<'a>(&'a self, pin: PinnedBundleLease) -> BoxFuture<'a, ()>;
}
```

`ScopedExecution::Actor` は、テナント、Sourceの登録・可視性改訂番号、現在のアクセス権を照合する。`System` は、登録済みSource、保持条件、Sourceトークン・エポック・DB有効期限を照合し、外部要求からは構築できない。P4の `RemoteOperation` は、一評価・一Sourceに限る封印済みRAM世代とRAMリースだけを返し、`Durable` 派生型には変換しない。P6の準備処理は `System + Durable` の事前検証を使い、イベント処理完了時は同じ検証を**トランザクション内で再実行**する。外部が任意に指定したキーを `pin_current` に渡して、未公開READYを自由に読めるAPIは作らない。

`pin_current` の永続化側の処理は、現在の世代ポインターをロックなしで暫定的に読み、先に字句索引の実体を事前検証する。その後、短いトランザクションでSource行をロックし、**その時点の現在の世代ポインター**と改訂番号を再取得する。変わっていれば上限付きで再試行し、同じならP7のREADY・成果物一式とP3 GraphのREADYを世代ロック下で検査して、DB時計の期限付きリースを同一トランザクションでINSERTする。Sourceロック中に字句索引のファイルI/Oを行わない。コミット後に字句索引を再確認し、欠損があればリースを解放して失敗させる。検証済みのキー・検証情報・リースだけを返す。旧有効化世代の現在のポインターは、CASの期待状態には使えるが、`ReuseCurrent` や世代固定の成功にはならない。字句索引ファイルとGraphの読み取りにはこのキーを使う。結果返却前に新しいトランザクションで、リース、同一のキー・ダイジェスト、ファイル・索引の可用性、利用主体・Source・項目・フィールドの現在の権限を再判定する。Graphの `REPEATABLE READ READ ONLY` スナップショットとP3読み取り検証を通す。失効や破損があっても別キーへ暗黙に切り替えず、結果全体を返さない。**現行のVersion/T10/Read/part/rawは常にSource正本で再判定**し、Projection/Graphに保存されたAccessProjectionやREADYで現在の権限を代替しない。

イベント処理完了時のロック順は、`outbox_events` の一行 → Sourceの一行 → `(source_id,generation_id)` 昇順のP7/Graph世代（同じキー内はP7→Graph）→ 対象キー順の完全構築/P3構築ガード → リースID順 → Searchイベント処理記録とする。手動再構築はoutbox行を取得せず、Sourceから始める。Source行を扱わないコピー・差分・検証バッチは世代 → ガードだけをロックし、後からSourceを取得しない。複数SourceはSource ID順に別トランザクションで扱う。Read Committedで連続SELECTのスナップショットが同一だと仮定せず、行ロックと条件付きUPDATEで保護する。

`complete_event_if_current` は、P6 outboxのトークンとDB有効期限、Sourceのトークン・エポック・DB有効期限、`last_published_epoch <= epoch`、期待する `(current key, manifest digest, bundle digest, pointer_revision)`、候補の同一キーのREADYとガードを、同じ短いトランザクションで再検査する。ポインター・改訂番号・最終エポックのCASと、`(source_id,event_id)` イベント処理記録のエポック単調な挿入・更新を、同じコミットに置く。同じエポックは、キー・射影ダイジェスト・成果物一式の版/ダイジェストが完全一致する場合だけ冪等とし、古いエポックは `Lost` とする。`ReuseCurrent` / `Unchanged` / `Duplicate` も、過去のイベント処理記録だけでは成功とせず、**今の現在世代のREADY実体**を照合する。増分公開成功時は同じコミットでP3ガードをDELETEし、完全構築の場合は完全構築ガードをDELETEする。CASで敗北した場合はポインターとイベント処理記録を書かず、ガードを保持して明示的な中止処理へ渡す。汎用outboxの `delivered_at` は、別トランザクションのP6ワーカーがトークンフェンス付きで更新する。手動再構築は未完了イベントに確認応答しない。

PGコミットの応答を失った呼び出しは `CompletionUnknown` とし、その呼び出しに成功を返さない。DB再接続後に、現在世代のREADY、ポインター改訂番号、イベント処理記録、フェンスを再読し、後続のフェンス付き再配送または明示的な照合で収束させる。Sourceリースの取得・更新の応答が不明な場合も、所有権を推測して処理権を取得しない。ロックのタイムアウト、`40001`、`40P01` はトランザクション全体をロールバックし、同じ期待状態・冪等性キーで有限回再試行する。毎回DB条件を再評価する。上限・カウンターのオーバーフロー・DB結果不明は明示的なエラーとし、CASの敗北を成功へ変換しない。

<a id="5-pinguardgc-と-crash-recovery"></a>
## 5. 世代固定・ガード・GCとクラッシュ復旧

- `retire_unpinned` / `discard_unpublished` / 期限切れガードの回収は、Sourceロックの下で、現在の世代、DB時計時点で有効な評価リース、完全構築ガード、P3ガードの**基底と対象の双方**を調べる。現在の世代、有効な世代固定、有効なガードのあるキーは削除しない。ガード失効を検出しても単に無視せず、同じロック順での回収を完了してからGCを再判定する。Searchイベント処理記録はGCを防ぐ世代固定ではない。
- 増分コピーは、P3の凍結どおり、整列した基底 `FOR SHARE` / 対象 `FOR UPDATE` → ガードのロック順とする。トークン・フェンス・DB有効期限・基底の変更不能な検証情報・対象のBUILDING状態・コミット済みカーソルを、バッチ開始時とコミット直前に確認する。Sourceポインター改訂番号が進んでも、有効なガードによるコピーを壊さない。完全構築・増分構築の対象がREADYになった後も、公開・中止までガードを保持する。期限切れトークンは、更新・コピー・READY化・公開に再利用できない。
- 期限切れ・明示的中止の対象が未公開かつ世代固定されていないと確認できたら、一つのトランザクションで、**P7とGraph対象を `DELETING` → 該当ガードのDELETE → 期限切れリースのDELETE → 対象の子行のDELETE → 対象世代のDELETE**とする。実際の外部キー制約を守る順序は、P3参加者 → 関係 → リソース → Graph世代、P7ペイロード・字句索引・検証情報 → P7世代である。DB外索引の削除はコミット後に行う。基底・対象世代のロックはコミットまで保持する。`ON DELETE RESTRICT` のガードを残して親を消さない。途中のエラーは全体をロールバックし、ガードと対象の両方を残す。現在の世代・世代固定と、期限切れガードが予想外に共存する場合は、整合性違反として削除しない。READYの基底は、ガードが消えた後、別のGCトランザクションでだけ退役候補になる。
- DB外索引は、**DBコミット後**に、冪等な孤立物の走査回収で削除する。コミット前に削除し、ロールバック後の現在世代を壊すことを禁止する。削除失敗は保護されない余剰バイト列として観測し、現在世代を再公開する理由にしない。準備用ディレクトリのクラッシュ残骸も、キー・DB状態・ガードを照合してから掃除する。外部ファイルの消失時にDBのREADYを根拠として成功扱いせず、該当キーを隔離・利用不可とし、Source正本から新しいキーへ完全再構築する。
- 再起動時は移行・ロール・登録情報を検査してから、現在世代のP1マニフェスト、全ペイロード、字句索引の再オープン・完全性照合・ツリーダイジェスト、P3 Graph検証情報、成果物一式の版と複合ダイジェストを復元検証する。Source行・READY・Graph・ファイルのいずれかに不一致があれば、そのSourceの検索・公開を安全側に倒して拒否する。中断したBUILDING、失効したガード・リース、確認応答不明は、DB時計とフェンスに従って回収する。有効なガードを勝手に消さず、進捗カーソルを検証できなければ別キーから再構築する。RAM上のRemote世代・カーソル・セッションは復元しない。バックアップ → 別DB復元でも、同じキー・ダイジェストと外部索引バイト列の復元を個別に検査し、DBの復元だけで「復旧」としない。

<a id="6-保持条件と後続実装の切り分け"></a>
## 6. 保持条件と後続実装の切り分け

永続化の準備処理は、`PersistableGenerationManifest` / `PersistableResourceProjection` と、登録Sourceの保持条件・フィールド許可の証明の双方を確認する。`NoRetention`・`SessionOnly` のRemote由来バイト列、Unit、Graph、索引、検証情報を、PG・ディスク・outbox・バックアップに永続化しない。`PersistentDiscoveryMetadata` は、本文由来Unit・埋め込みの保持許可ではない。項目のSource・所有者・スコープ・改訂番号・リースが不明または失効していれば拒否する。P4の `NO_RETENTION` は評価用RAMと短命の開示に閉じ、P7の最終構成は、ログ・テレメトリー・スプール・コアダンプの漏出防止ゲートを別途満たす。型付き設定、秘密情報の解決処理、Audit/OTel、HTTPと送出寿命、配備、SLOは、[P7早期契約](p7-runtime-contract-draft.md) §§2,5–8と最終P7設計で扱う。この共有基盤のREADYだけで、それらを完了扱いしない。

| 先行作業・書き込み担当 | 完了判定に必要な局所証拠 |
| --- | --- |
| P6-I03 → P7 `0002+` の移行書き込み担当 | 独立したSQLx台帳、Source行が一つであること、複合外部キー、READY・検証情報・ペイロード・リース・ガードのCHECK・FK・トリガー・ロールを実PGで確認する。既存Domain行と台帳は不変。 |
| P7永続成果物の書き込み担当 | 強制終了・再オープン後のProjection/Unit/網羅性DTOの再計算、実字句索引文書の完全性照合とツリーダイジェスト、Graph対応付け、成果物一式の重複・行やファイルの破損時の安全側の拒否、v1複合ダイジェストの基準検証ベクトル。 |
| P3/P7調整処理の書き込み担当 | 完全構築・増分構築ガードの登録・フェンス、コピーとGCの競合、CAS・世代固定・失効、`DELETING→guard DELETE→children DELETE→generation DELETE` と途中障害のロールバックを、独立プロセス・PGで確認する。 |
| P6-S03と接続処理の書き込み担当 | outbox → Source → 世代 → ガード → リース → イベント処理記録のコミット・ロールバック、古いエポック、同一エポックのダイジェスト衝突、公開コミット結果不明、別の確認応答結果不明、GC済みの過去イベント処理記録の再配送を、実PGで確認する。 |
| 最終P7組み立ての書き込み担当 | P3の測定・選定検証記録、P6の汎用配送とSearch配送の縦断検証記録、P1本文の完全性照合と網羅性、P4のスコープ付きRAM、P5の最終アクセス確認・APIを集約する。再起動・別DB復元・複数プロセス、読み取り・公開・GC、保持・観測・稼働状態、正確なheadのゲートを別途記録する。 |

**公開前の未確定条件：** P3-P04の独立監査NO-GOを、追加PoCと再審査で解消する必要がある。P1 Graph入力ダイジェストとP3 Graph内容ダイジェストを再計算できる `GraphReceiptMappingV1` のエンコーダーとテストベクトルも、実装・検証が必要である。非PG Graphを選んだ場合は、上記原子性に代わる審査済みプロトコルが必要となる。これらが成立していない分岐は、READY・公開を停止する。その間も、共有PG調整基盤のスキーマ・ポート、全体共通の所有権台帳、P6 Sourceリース、P1永続ペイロード、世代固定・ガード・GCのうち、Graphに依存しない局所実装は進められる。

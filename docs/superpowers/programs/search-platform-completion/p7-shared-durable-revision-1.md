<a id="p7-共有-durable-generation-基盤--設計改訂-1"></a>
# P7 共有永続世代基盤 — 設計改訂 1

[翻訳元の固定公開原文（commit 0ecf486719e3c9d71242e289a7564ad6d1032b3c）](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-shared-durable-revision-1.md)

本書は意味保存の日本語訳であり、原設計の再承認や資格の追加ではありません。既存の承認ハッシュは当時の原文・証拠を指し、訳文のハッシュではありません。以下の状態・手順・次の作業は当時の記録であり、現在の実行許可ではありません。最新の状態は[実行状態の正本](../../execution/search-platform-completion-program-status.md)を参照してください。旧見出しアンカーは明示IDで維持しています。Searchイベント処理記録（Search receipt）はDB上のイベント処理記録を指し、工程の検証記録・証拠（receipt）とは区別します。

状態：**DRAFT（草案）/ 独立再審査・実装計画への反映・実DB検証前**（2026-09-30）。[元設計](p7-shared-durable-design.md) SHA-256 `99a989e30ec77ea5f79915a3db22ecd31b910decbabc825c2ae8bca6f1041a3b` は保存し、[独立レビュー](p7-shared-durable-review.md)の5件だけを本書で補う。抵触時は、本書を元設計 §§2–5の該当条項に優先する。P1/P3/P6の凍結、P3構築ガードと外部キー制約を守る回収、P1 Archiveとの結び付けは引き続き拘束する。本書は、本番コード、Graphバックエンド選定、P6の全縦断、P7の最終ランタイム・HTTP・運用の完了記録ではない。

| レビュー指摘 | 本改訂の拘束箇所 |
| --- | --- |
| 1. イベント・Sourceエポックに結び付いていないREADY候補 | §1の `stage_origin`、完了トランザクション、`ReuseCurrent` |
| 2. 完全構築ガードと対象の変更不能な結び付け、および失効フェンスの不足 | §2の対象との結び付け、DB DMLフェンス、回収 |
| 3. P3の `stage_full` にある別コミットの経路 | §3のトランザクションに結び付いた登録と本番ポート |
| 4. 永続的な世代固定に所有者スコープがない | §4の変更不能なスコープ参照と現在状態のゲート |
| 5. Source種別とRemote期待登録集合の正本性 | §5の種別制約、ホストスナップショット、全体共通の直列化ロック |

<a id="1-event-candidate-origin-と完了条件"></a>
## 1. イベント候補の起点と完了条件

`search_generation` の登録時に `stage_origin IN ('EVENT','MANUAL')` を固定する。`stage_event_id` と `stage_source_epoch` は、**EVENTの場合だけ両方をNOT NULL**とし、MANUALの場合は両方をNULLとする。`stage_source_epoch > 0` とし、`stage_origin`・イベントID・エポックはINSERT後に変更不能とする。構築側ロールには更新権限を与えない。`source_id`、世代キー、`source_snapshot`、`activation_epoch`、マニフェスト・キー・ガードとの結び付けも、同じ登録トランザクションで固定する。EVENT登録では、非公開の `EventCandidateHandle` とP6の `SearchDeliveryFence` を要求する。ロック済みoutboxイベントの設定済みSource経路、イベントID、現行Source所有者トークン・エポック・DB有効期限を照合してから登録する。MANUAL登録は別の非公開の `ManualBuildHandle` とポートを使い、outbox行を取得せず、EVENTハンドルにも変換できない。どちらのハンドルも、SQLxに依存しないアプリケーションの不透明な値であり、HTTP・プロバイダー・通常の構築側が任意値から生成することはできない。

`0002` の `search_generation` 生成SQLは、少なくとも以下の項目完備性CHECKを含む。既存の `stage_event_id?`/`stage_source_epoch?` を、単なる任意列のまま残さない。

```sql
stage_origin text NOT NULL CHECK (stage_origin IN ('EVENT','MANUAL')),
CONSTRAINT stage_origin_complete CHECK (
  (stage_origin = 'EVENT' AND stage_event_id IS NOT NULL
    AND stage_source_epoch IS NOT NULL AND stage_source_epoch > 0)
  OR (stage_origin = 'MANUAL' AND stage_event_id IS NULL
    AND stage_source_epoch IS NULL)
)
```

`CompleteEventRequest` は、概念上、`PublishCandidate { event_candidate_handle, expected_current, verified_bundle }` と `ReuseCurrent { expected_current, expected_manifest_digest, expected_bundle_digest }` に分ける。元P6-S01の単一 `candidate` フィールドを、本番での無条件入力として流用しない。`PublishCandidate` は、outbox行 → Source行 → P7/Graph世代キー順 → ガード → リース → イベント処理記録の既定ロック順を守る**一つの短いPostgreSQLトランザクション**で、次を再検査する。

1. Outboxリースのトークン・DB有効期限・未完了状態、Source所有者トークン・`fence_epoch`・DB有効期限、イベント経路の `SourceId`、所有権のACTIVE・テナント・有効化状態を照合する。Sourceリースの `epoch` とoutboxの `event_id` はDB行から取得し、呼び出し側のハンドルだけを信用しない。
2. `search_generation.stage_origin='EVENT'`、`stage_event_id=outbox.event_id`、`stage_source_epoch=Source.fence_epoch`、候補のSource・キー、保存された `source_snapshot`、P1マニフェスト・全ペイロード・成果物一式の検証情報のスナップショット・キー、`activation_epoch=Source/ownership/identity` を完全に一致させる。完全構築または増分構築の**その対象**に発行したトークン・フェンスが、保存された対象との結び付けおよびガード行に一致し、DB時計で未失効であることも照合する。外部の字句索引を事前検証した後も、DB内のREADY・Graph READYと、変更不能な成果物との結び付けを再読する。異なるイベント、古いエポック、MANUAL起点、別のSource・有効化世代・スナップショット・ガードの場合は、ポインターとイベント処理記録を一切書かず、`Lost` または明示的な整合性エラーとする。
3. 期待する現在状態のキー・二つのダイジェスト・改訂番号と、`last_published_epoch <= epoch` を条件としたCASを行う。同じコミットで、`(source_id,event_id)` イベント処理記録を、P6のエポック単調・同一エポック完全一致規則に従って書く。CAS成功時に該当ガードだけをDELETEする。失敗時はロールバックし、ガードを維持する。汎用の `delivered_at` は、P6の後続のフェンス付き確認応答だけに委ねる。

`ReuseCurrent` は、**候補の準備状態を受け取らない**。同じoutbox/Sourceフェンスのトランザクションで、その時点のSourceの現在の世代ポインターが `expected_current` と、キー・マニフェスト/成果物一式のダイジェスト・改訂番号で一致し、同一Source・有効化世代のP7 READY、P3 READY、P1成果物一式、外部成果物を検証できる場合だけ、その現在の世代キーをイベント処理記録に記録する。MANUALキーを任意候補として渡し、公開する経路はない。元の過去イベント処理記録やGC済みキーは証拠にならない。現在世代のREADYが消えた場合、別の有効化世代に属する場合、成果物が利用できない場合は、`Unchanged`/`Duplicate` を返さず、再読・再構築へ進める。手動再構築自体は未完了イベントに確認応答しない。`ReuseCurrent` に完全構築・増分構築ガードは要求しないが、現在のREADY実体と、Source正本の現在のアクセス権・保持条件のゲートは省かない。

DBは、`stage_origin` の許可リスト、イベント・エポックの「全項目完備または全項目NULL」のCHECK、変更を禁止するトリガー、登録・完了専用ロールで、上記経路を裏付ける。`PublishCandidate` の再検査とポインター・イベント処理記録の書き込みを、別接続・別トランザクションに分けない。コミット応答不明は元設計の `CompletionUnknown` であり、成功に変換しない。

<a id="2-full-target-の不変-guard-binding-と失効-fence"></a>
## 2. 完全構築対象とガードの変更不能な結び付け、および失効フェンス

P7の `search_generation` に、`build_kind IN ('FULL','INCREMENTAL')`、`full_guard_token UUID`、`full_build_fence BIGINT` を追加する。FULLはトークンとフェンスが両方とも非NULL、かつフェンス > 0、INCREMENTALは両方NULLとする項目完備性CHECKを設ける。FULL行の `(source_id,generation_id,full_guard_token,full_build_fence)` にUNIQUEを置き、`full_guard_token` と `(source_id,full_build_fence)` にも一意制約を置く。`search_generation_full_guard(source_id,target_generation_id,guard_token,build_fence)` から対象の4列へ、`ON DELETE RESTRICT` 複合外部キーを張る。ガードが別対象のトークン・フェンスを借りることを許さない。対象の構築種別・トークン・フェンスは、登録後、FAILED・DELETING・READYを含めて変更不能とする。P3増分構築の対象・基底の結び付けと同じSourceの `build_fence_seq` を、Source行ロック下で一度だけ増やす。SQLのCHECK・FKだけでは時刻を保証しないため、以下をDBトリガー・ロール・非公開ポートの三層で強制する。

```sql
CONSTRAINT full_binding_complete CHECK (
  (build_kind = 'FULL' AND full_guard_token IS NOT NULL
    AND full_build_fence IS NOT NULL AND full_build_fence > 0)
  OR (build_kind = 'INCREMENTAL' AND full_guard_token IS NULL
    AND full_build_fence IS NULL)
),
FOREIGN KEY (source_id,target_generation_id,guard_token,build_fence)
  REFERENCES search_generation
    (source_id,generation_id,full_guard_token,full_build_fence)
  ON DELETE RESTRICT
```

後半の複合外部キーは `search_generation_full_guard` 側の定義、前半のCHECKは `search_generation` 側の定義である。両表の行を同じトランザクションで作り、対象だけ、またはガードだけをコミットしない。

- 完全構築対象のP7ペイロード・検証情報・字句索引の子表DMLは、親のBUILDING行を `FOR UPDATE` でロックする。対応する完全構築ガード行もロックし、**対象に保存されたトークン・フェンスとの一致**と `expires_at > clock_timestamp()` を確認する。P3 Graphを接続した場合、その完全構築のGraph親行とリソース・関係・参加者のDMLにも、同じガード・結び付けのフェンスを適用する。通常の構築側は、ガードのINSERT/UPDATE/DELETE、対象との結び付けのUPDATE、ガードなしの親行のINSERTを直接実行できない。バッチはコミット直前にもDB時計を確認し、期限切れなら全体をロールバックする。
- 信頼された各バッチポートは、`FullBuildHandle` のSource・キー・トークン・フェンスを、対象とガードの保存値に照合する。トリガーもハンドルを信用せず、DBの親行・ガード・有効期限を検査する。通常の構築側へ与える直接の子行DML権限は、このトリガーの下でだけ許可し、ガード失効後の直接SQLも拒否する。
- `validate_ready` は、P7/Graph対象とガードを同じ接続・既定のロック順で再検査し、トークン・フェンス・DB有効期限と全検証情報を確認してからREADYにする。Graph READYとP7 READYは、それぞれの実体検証を必要とし、一方のREADYだけでは公開できない。READY後の子行変更は、ガードの有無にかかわらず拒否する。
- `renew_full_guard(handle, bounded_ttl)` は、Source → 対象 → ガードの一トランザクションで、保存トークン・フェンスとの一致と未失効をDB時計で確認した場合だけ延長する。失効済みガードは復活させない。`publish_if_current` と§1のEVENT公開は、ポインターCASのトランザクション内で同じガード・DB有効期限を確認し、成功したCASと同じコミットでガードを消す。CASで敗北した場合はガードを残す。
- 期限切れ・中止では、Source → 対象世代 → ガード → リースをロックし、現在の世代でも固定中でもないことを再確認する。同じトランザクションで、P7/Graph対象を `DELETING` → **一致したガードのDELETE** → Graphの参加者・関係・リソースとP7子行のDELETE → Graph/P7親行のDELETEの順に処理する。失敗時は、ガードと対象を含めて全体をロールバックする。失効後、同じ対象へガードを再発行しない。回収成功後も恒久的な `search_generation_identity` がキー再利用を拒否するため、新しい構築には新しい世代キーとフェンスが必要となる。

Graphの完全構築の親行にも、同じ変更不能なトークン・フェンスを記録する。選定後のPG Graphスキーマでは、P7対象への複合外部キーと変更用トリガーを適用する。Graphが未選定の間は、P7のうちGraphに依存しないスキーマ・ポートの局所実装だけを許し、Graph READY・公開は無効のままにする。別バックエンドを選ぶ場合は、対応する原子的なフェンスプロトコルの独立審査まで接続しない。

<a id="3-p3-stage_full-と登録-transaction-の唯一の入口"></a>
## 3. P3の `stage_full` と登録トランザクションの唯一の入口

本番の完全構築登録は、P7調整担当が所有する一接続・一トランザクションに限定する。EVENTの場合はoutbox行を最初にロックし、MANUALの場合はSource行から始める。その後は、Source `FOR UPDATE` → 所有権・有効化状態・保持条件の検査 → 新しい識別情報のINSERT → 完全構築対象のP7 BUILDINGのINSERT →（選定済みPG Graphの）Graph BUILDINGのINSERT → 完全構築ガードのINSERT → コミット、という順序で行う。Sourceスナップショット、マニフェスト、有効化世代、同じキー・トークン・フェンスをP7/Graph親行に保存する。**PG Graphを本番接続する場合は**、Graph行とガードの両方が揃っていない状態を外部へコミットしない。Graph INSERT・ガードINSERTの失敗、キー衝突、フェンスのオーバーフローは全体をロールバックし、無保護の対象が見える状態を残さない。Graph行は、P7の非公開のトランザクションに結び付いた `GraphRepository::register_full_on(&mut PgConnection, registered_target)` のようなメソッドだけが作る。別プール・別接続で `stage_full` に親行を作らせない。

P3-G01/G04の本番ポートは、上記コミットで発行した `RegisteredFullBuildHandle` を入力とする `stage_full_registered(handle, resources, relations, ...)` 相当へ制限する。このポートは、既存のBUILDING親行に**バッチの子行だけを追加**し、各バッチでP7/Graph親行と§2の完全構築ガードをDBで再検証する。ハンドルのフィールドは非公開とし、DBを再読せず、ハンドルを持っていることだけで許可しない。P3計画の旧 `stage_full(manifest,retention,...)->GraphStage` が自律的に親行をINSERTする形式は、隔離PoC・フィクスチャ専用の別ポートとする。本番の構成起点へエクスポート・接続せず、本番DBロールに親行のINSERT権限を与えない。P3の `stage_incremental` は、凍結されたガード追補の基底・対象の手順を保ち、共有対象の登録だけを、P7の同一トランザクションに結び付いた接続へ統合する。P3-G01/G04/C01の実装計画に、このシグネチャとロール・コンストラクターの境界を明示してから着手する。

P3-P04のバックエンド再判定と、`GraphReceiptMappingV1` のP1ステージング入力・P3内容の二つの正規化エンコーダーおよび基準検証ベクトルが成立するまでは、Graphの本番移行、Graph READY、共有公開を有効にしない。P3隔離PoCの成功を、この登録経路の検証とみなさない。

<a id="4-durable-pin-の保存された発行先"></a>
## 4. 永続的な世代固定に保存する発行先

`search_evaluation_lease` は、元設計のキー・評価・有効化世代・二つのダイジェスト・DB有効期限に加え、`tenant_owner_key NOT NULL`、`actor_scope_ref NOT NULL`、`registration_revision NOT NULL`、`visibility_revision NOT NULL`、`access_revision NOT NULL` を保存する。正数の改訂番号と、長さを制限した非秘密の参照をCHECK・ポートで検査する。世代固定のINSERT後は、識別情報・スコープ・参照・ダイジェストの全欄を変更不能にする。`actor_scope_ref` は、信頼されたホストアダプターが `TrustedSearchScope` と `TrustedDiscoveryBinding` の利用主体・セッション・評価スコープに対して発行・再照合する、非秘密の不透明な参照である。認証主体、セッション認証情報、アクセスハンドル、本文、トークンをDBに保存しない。リースIDや評価IDを持っているだけでは、参照元の利用主体を証明できない。

この参照は、リースTTL内なら別プロセス・再起動後も、ホストの正本が同じ信頼された利用主体・評価スコープへ再解決できる必要がある。解決不能なら、その世代固定を安全側に倒して拒否する。プロセス内ポインターや `Instant` のバイト表現は保存しない。DBは、改訂番号が正数かつBIGINT範囲内であること、参照の長さ上限と非空を検査し、スコープフィールドの変更を禁止するトリガーを持つ。

`pin_current` は、渡された `AuthorizedSourceScope` のテナント・利用主体スコープ参照・登録/可視性/アクセス改訂番号と、Source/所有権のACTIVE・テナント・有効化状態を、現在状態のゲートで検査する。同じトランザクションで、それらの変更不能な値をリース行へINSERTする。`renew_pin`、`verify_pin_before_return`、利用主体向けの `release_pin` は、**世代固定と、現在の信頼された `AuthorizedSourceScope`/評価の両方を受け取る**シグネチャに改める。保存行をリースIDでロックし、所有テナント、Source・キー・評価、利用主体スコープ参照、登録/可視性/アクセス改訂番号、有効化世代、二つのダイジェスト、DB時計による有効期限を一致させる。そのうえで、P4の `CurrentSourceVisibilityPort` とホスト台帳、P5の現在の利用主体・Sourceゲートを再実行する。`verify_pin_before_return` では、返却対象の項目・フィールド・Graph参加者ごとのP5最終ゲートと、DocumentのVersion・Part・原データ・現在のReadも再実行する。P5のSource種別に依存しないカタログ改訂は、既存P4の `TrustedSearchScope`/`AuthorizedSourceScope` の発行処理を再利用し、P7専用の第二の利用主体・Source発行処理を作らない。Denied・Unknown・期限切れ・Source再登録・可視性変更の場合は、更新と結果返却を拒否し、別の利用主体・テナントによる同じリースIDを受け付けない。失効後の利用主体向け解放でも成功を偽装せず、失効リースの物理的な掃除は、限定された調整担当GCロールが別経路で行う。旧世代を固定していても、Sourceポインターが別キーへ進むこと自体は失効条件にしない。ただし、保存されたキー・検証情報と、Sourceの現在の正本性は必ず検査する。RemoteのRAMリースはこの表に置かない。

<a id="5-global-source-kind-と-remote-desired-集合の権威"></a>
## 5. Source種別の全体共通管理とRemote期待登録集合の正本性

`search_source_ownership.source_kind` は、移行で `CHECK (source_kind IN ('DOCUMENT','REMOTE'))` を持つ。`source_id`、`tenant_owner_key`、`source_kind` はACTIVE/TOMBSTONEDを通じて変更不能であり、UPDATE/DELETEトリガーとロールへの権限付与で保護する。同じSourceIdを別種別や別テナントへ再登録しようとした場合は、削除済み記録化の後も拒否する。Document登録とRemote照合・同期は、**同じ `search_registration_serial` の一行を最初にロック**し、同じ全体共通の所有権・Source台帳とロック順を使う。Document登録も、このロックの外で独立したカタログを確定しない。

現行P4の `SourceRegistrationLedgerPort::reconcile(&BTreeMap<SourceId, RemoteSourceRegistration>)` の対応表だけでは、「全テナントの全Remote」である証明がない。[P5のSource種別に依存しない改訂2](p5-api-contract-revision-2.md) §2の `SourceRegistration::{Document,Remote}`、`RegistrationNamespace`、`CompleteDesiredRegistrations { namespace, deployment_revision, set_digest, registrations }`、`SourceRegistrationLedgerPort::reconcile` を、**P7本番アダプターの一つの契約**に採る。現行P4のRemote専用トレイトは、この移行後に本番の別台帳・別ポートとして残さない。両派生型の `SourceAuthorityDescriptor` は各登録から導出するビューとし、第二の書き換え可能な正本性の記録や、第二の利用主体・Source発行処理を作らない。`CompleteDesiredRegistrations` は、信頼された構成起点のホストが所有する完全な期待登録スナップショットからだけ構築する。`namespace=Remote` なら全テナント・全Remote、`namespace=Document` なら全テナント・全Documentの対応表を持つ。プロバイダー・要求や、呼び出し時の対応表自身から、完全性の正本性を作り出さない。

`RegistrationSetDigest` の正規化エンコーダーは、名前空間・版のドメイン分離子（Remoteは `remote-desired-set:v1`）を使い、長さを明示して区切った各項目を、SourceIdのUUIDバイト列の昇順で使う。テナント、`SourceKind`、SourceId、登録・可視性改訂番号と、各派生型の**サーバーが所有する全DTOフィールド**を、固定のタグ・順序・長さで符号化する。Remoteでは、プロバイダー・接続先・モード・正本性・リソース種別・アクセス・保持条件・鮮度・系譜・上限を含める。Documentでは、アダプター参照・許可種別・ローカルモード・列挙・保持条件を含める。JSONBの物理バイト列や部分的な対応表のハッシュは、正本のダイジェストにしない。アダプターは、期待登録集合の名前空間・全キー・全派生型・全DTO値を、ホスト正本の**同じ改訂番号の完全スナップショット**と厳密に等価であることを照合する。双方で正規化ダイジェストを再計算し、`set_digest` と一致させる。ホストの正本は、同じ信頼された構成起点の登録正本であり、別の利用主体・Sourceの信頼情報発行処理ではない。テナント・種別・改訂番号・可視性が一つでも異なる場合、一部テナントだけの対応表、余分なキー、登録の欠落、スナップショット未確定の場合は、**変更前に一つの原子的な失敗**とする。

`search_registration_serial` は、全体共通の一つのロック行を維持し、DocumentとRemoteの**名前空間ごと**に `deployment_revision` と完全な期待登録集合のダイジェストを保存する。アダプターはこの行を最初にロックし、ホストの該当改訂番号・ダイジェストがコミット直前にも現在有効であることを再照合する。保存改訂番号より古いもの、同じ改訂番号で異なるダイジェストは拒否する。同じ改訂番号・同じダイジェスト・同じ対応表は冪等とする。新しい改訂番号だけを一つのトランザクションで適用し、削除済み記録にする対象を、`source_kind = desired.namespace` の既存行に厳密に限定する。特にRemoteの照合・同期では、完全なRemote対応表にないRemoteだけを削除済み記録にし、`DOCUMENT` 行を条件に含めない。Documentの照合・同期もRemote行を変更しない。同名SourceIdが反対の名前空間の期待登録集合にあれば、種別衝突として全体をロールバックする。直列化行の該当改訂番号・ダイジェストと、所有権・Sourceの有効化世代・フェンスの変更は同じコミットとする。両名前空間の同時登録は、全体共通の直列化ロックで直列化する。照合・同期後も、`is_current` は保存DTO全体と現在の有効化状態・可視性を照合する。ホストの完全スナップショットを検証できない配備では、本番起動を拒否する。

<a id="6-計画と検証-gate"></a>
## 6. 計画と検証ゲート

P7実装計画、P6-S03と接続処理、P3-G01/G04/C01の該当作業に、上記のシグネチャ・スキーマ・ロール・ロック条件を反映する。名前を明示した各ケースは、実PostgreSQL、実際の構築側・調整担当・読み取り側ロール、独立接続、必要な同期障壁・障害注入を使って、**RED→GREEN**を残す。SQLx 0.9.0の固定済み `Migrator::dangerous_set_table_name` をSearch台帳に使い、独自の移行処理は追加しない。P6-I03のSearch `0001` 書き込み担当、P7の `0002+` 書き込み担当、Domain/Graphの別台帳は、元設計どおりとする。

| 指摘 | 名前を明示した必須の実PG回帰試験 |
| --- | --- |
| イベント起点 | `wrong_event_candidate_cannot_publish_or_write_receipt`; `old_epoch_or_manual_candidate_cannot_complete_event`; `reuse_current_requires_current_ready_after_gc` |
| 完全構築ガード | `expired_full_guard_rejects_late_child_write_and_ready`; `full_target_guard_cannot_be_reissued`; `guard_delete_then_child_failure_rolls_back_full_target` |
| Graph登録 | `full_registration_is_one_commit_with_guard_and_graph_key`; `graph_stage_failure_leaves_no_visible_unprotected_target`; `isolated_stage_full_cannot_publish_without_p7_guard` |
| 世代固定のスコープ | `pin_cannot_transfer_between_actor_scopes`; `registration_or_visibility_change_revokes_old_pin_read`; `foreign_tenant_cannot_renew_same_source_pin` |
| 所有権・照合同期 | `remote_reconcile_never_tombstones_document`; `partial_tenant_map_cannot_tombstone_foreign_remote`; `document_remote_same_source_id_is_rejected_after_tombstone`; `partial_or_stale_remote_desired_set_is_atomic_failure` |

さらに、P1複合ダイジェストv1の基準検証ベクトル、実字句索引Unitの双方向完全性照合・復元、P3の `GraphReceiptMappingV1` のステージング入力と内容の両エンコーダーベクトル、コミット結果不明、現在世代・世代固定・ガード・GCの競合、旧所有権行の移行・遡及補完の失敗、別DB復元を、対象を絞った検証記録に残す。P3-P04の再審査前は、Graphに依存しない共有スキーマ・ポートだけに限る局所判定とし、Graphの本番接続や最終P7 GOと混同しない。

<a id="p7-production-runtime--design-revision-2"></a>
# P7 本番ランタイムの設計改訂2

[固定された公開原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-design-revision-2.md)に対応する意味保存の日本語訳です。原設計の再承認、実装・実行時の適格性検証の追加ではありません。既存ハッシュは当時の原文・証拠を指し、訳文のハッシュではありません。以下の状態・未達条件・次の作業は記録当時のものです。[最新の実行状態](../../execution/search-platform-completion-program-status.md)と[統合後の凍結記録](p7-runtime-freeze.md)を併せて確認してください。過去の手順を現在の実行許可として扱わないでください。

状態：**REVISED PROPOSAL（改訂提案）/ 独立したアーキテクチャ・セキュリティ再審査待ち / 設計凍結前**（2026-10-01）。[改訂1](p7-runtime-design-revision-1.md)の独立再審査で残った、ホスト登録情報一覧の公開処理と、Search/systemのAuditイベント生成処理の2件を具体化する差分です。抵触する箇所は本書を優先し、それ以外は改訂1と[元設計](p7-runtime-design-completion.md)を維持します。設計GO、実装、本番の適格性検証、SLO保証、マージ、本番デプロイの記録ではありません。`spec/`が規範の正本です。

## 1. 固定入力と現在の欠落

作業ツリーは `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff`。下表は執筆時のSHA-256であり、未コミット文書の内容の同一性を示すものです。承認済みの設計凍結を意味しません。

| 入力 | SHA-256 |
| --- | --- |
| `p7-runtime-design-revision-1.md` | `422e00dcb0c685feb2fc9f81755e25bdd555d2e4b7602ccadb85e4c281a9d035` |
| `p7-runtime-plan-revision-1.md` | `d24cba464e40f9d07d2baf9095d4ece1bb099f0f8b06cfaca6eee2e2982e3d57` |
| `p7-runtime-architecture-review-revision-1.md` | `a6f91b67ecfd220191486aa2cdd6715967a8115f9e0b9dc6aee8ecaee6c19e29` |
| `p7-runtime-architecture-review.md` | `0bf9ce336e664a3eb4a5e14c7f37ec9a351bc4edbfa9d6b8b6435f002088f793` |
| `p7-shared-durable-freeze.md` | `20b5b64ac6c8e6209a3618e1c8f4577f1f48333991df0cffe9af96e2cbd5e110` |
| `p7-shared-durable-plan.md` | `97c122bf4447dd63caeac24930503247c11d52e533e1825d913f737162812517` |

当時の `crates/search-application/src/source_registration.rs:675–813` のホストポートは、完全性を示す名前を付けたコンストラクターと合成フィクスチャまでで、本番用の公開処理はありません。同じ箇所の `ServerDocumentRegistrationConfig` と `crates/search-application/src/remote_registration.rs::ServerRemoteRegistrationConfig` は、既存のサーバー管理下の入力型です。現行Documentの `audit_outbox_events` は `resource_id UUID NOT NULL`、`resource_type` は `Document|Folder|AccessPolicy` に限定されます（`0001_document_authoritative_core.sql:86–100`、`0006_document_management_access_v0.sql:55–57`）。Search Sourceやsystemを `AccessPolicy`/nil UUIDに偽装せず、専用の型付き生成元を追加します。

<a id="2-host-registration-ssot-と-atomic-publisher"></a>
## 2. ホスト登録情報の正本と原子的な公開処理

**発行元を、P7-R01Pが所有する `search-runtime` のホスト登録情報公開処理に固定します。** R01の信頼された運用者による登録入力を唯一の作成用入力とし、`ServerDocumentRegistrationConfig`、`ServerRemoteRegistrationConfig`、登録0件のテナントも含む独立した `tenant_roster` を、版管理された一つの `HostRegistrationInputV1` として受け取ります。これは既存のSearchアプリケーションのサーバー管理下DTOとランタイム構成境界を使う、新しい実装責務です。現行リポジトリに本番用のホスト公開処理が実在するという主張ではありません。リクエスト・プロバイダー、SearchのSource/所有権/現在値の行、テナント別クエリ、手編集した部分的な対応表、合成ホストは、入力や完全性の証明に使いません。ホストが全テナントを列挙する正本を提供できない配備では、安全側に倒して拒否します。外部の識別サービス、実際のホスト、認証情報は選びません。

P7-R01Pを `crates/search-runtime/src/host_inventory_publish.rs` とSearchマイグレーション `0004_host_registration_inventory_v1.sql` の唯一の実装担当とします。ホスト管理下の登録情報一覧の変更不能な改訂行と、単一の現在headを、P7のSearch Source台帳とは別の表・別のロールに置きます。各改訂には、配備エポック、単調増加する正本の改訂番号、独立したテナント一覧、Document/Remoteのテナント別完全集合（空集合も明示）、各名前空間の登録改訂番号・正規ダイジェスト、テナント一覧のダイジェスト、全体ダイジェスト、書き込み元の来歴、スキーマ版を含めます。Documentは実接続済みアダプターの能力を証明する仕組み、Remoteはサーバー管理下の登録検証を通します。全項目のテナントが一覧に属すること、各テナントの宣言件数と実際の行集合、全DTOフィールド、全体でのSourceId一意性を検査します。認証情報/SecretRefの値は登録情報一覧に保存しません。テナント一覧の完全性は、登録集合自身やダイジェストの自己一致から導きません。ホスト作成用入力に含まれる独立したテナント一覧を規範集合として検査します。ホスト入力がその規範集合を提供できない場合は、公開を拒否します。

公開処理の**一つのPostgreSQLトランザクション**で、変更不能な改訂、全テナント・二つの名前空間の行、テナントごとの件数/ダイジェスト、現在headに対する旧改訂番号を条件としたCAS（比較交換）、§3の `host.registration.changed` Audit行を書き、コミットします。一方の名前空間・一つのテナントだけの更新を見える状態にしません。同じ改訂番号で内容が異なる場合、古い改訂、エポック不整合、SourceId衝突、Audit INSERT失敗では、すべてロールバックします。コミット応答が不明なら、再接続した独立接続でhead/全行/ダイジェスト/Audit event_idを再読し、一致を証明するまでリスナーと処理権取得を閉じます。現在headはホスト登録情報一覧への参照であり、P7のSource現在値ポインターや第二のSource台帳ではありません。R01の設定は、この登録情報一覧の参照と入力改訂番号を保持します。ファイルの直接再読み込みや監査なしの切替を、利用主体から見えるカタログへ反映しません。

P7-R02の `host_registration.rs` は、公開処理に対する読み取り専用アダプターです。変更不能な改訂と現在headを同じ読み取りトランザクションから取得し、ホスト作成用入力のテナント一覧/改訂番号と、別接続で再読した改訂/全行を突き合わせます。`CompleteDesiredRegistrations::capture(..., Document/Remote).await` の二つの結果が同じエポック・正本改訂番号・テナント一覧に属し、名前空間ダイジェストとDTOの厳密な一致を確認してから、P7-02が所有する、`PgPool` に基づく唯一の `SourceRegistrationLedgerPort` へ渡します。`SourceRegistrationCatalog::try_new(...).await` の後、ホストheadとP7の台帳/所有者/種別/有効化状態/現在値を再読します。P7-12の現在状態走査を通してから、P5-08の四つのルートとP6-I04の処理権取得を開きます。再読み込みは、新しいカタログ全体の検証後に一度だけ切り替えます。途中失敗、再起動後の不一致、古い改訂では、旧カタログの現在状態ゲートも未確定として閉じます。R02は公開処理/Source台帳/ルート生成処理を複製しません。

<a id="3-searchsystem-typed-audit-source-と-producer"></a>
## 3. Search/system用の型付きAudit生成元とイベント生成処理

R04Aは、既存Documentの `audit_outbox_events` のイベント識別・対象に関する制約を緩めず、Domainの `0010_audit_delivery_v0.sql` に**別の**追記専用 `search_audit_outbox_events` を追加します。両生成元は同じPostgreSQL内で別の表・別の配送状態を持ちます。R04AのAudit専用配送処理が、生成元タグ付きイベントを版管理されたデコーダーとクラス別の許可リストにより、同じリレーショナルAudit保存先へ投影します。P6 Domainの `outbox_events.delivered_at`、Searchイベント処理記録、各Auditの `delivered_at` は独立しています。R04Aの既存のリース/フェンス、上限付き再試行/DLQ、保存先の `event_id` 一意性、コミット結果不明時の再読契約を、両Audit生成元に適用します。保存先に同じevent_idで内容が異なるものがあれば、一致扱いせず停止して調査します。

Search生成元の行は、`event_id UUID`、`schema_version`、閉じた許可集合である `event_class`/`event_type`/`origin_component`/`actor_kind`/`subject_kind`/`result`/`reason_code`、UTCの `occurred_at`、必要な場合だけの上限付きで安定した `actor_ref`/`subject_ref`、Audit専用の配送状態を、型とSQL CHECKで拘束します。任意の `data JSONB`、生のトークン/クエリ/コンテンツ/プロバイダーの所在情報、SecretRef、具体的な欠落内容は持ちません。`actor_kind=VerifiedPrincipal` は既存ホスト検証処理が返す非秘密の参照だけとし、`SystemComponent` は固定列挙型とactor_ref NULL、未認証の拒否は `Unknown` とactor_ref NULLにします。`subject_kind=SearchSource` は、現在の権限で対象を特定できる管理操作だけが、閲覧を制限したAuditに参照を持ちます。未知・不可視対象の拒否は `UnknownTarget`/参照NULLにします。`RuntimeConfig`、`AuditPolicy`、`MaintenanceOperation`、`SystemComponent` は、それぞれ固有の型付き対象であり、Document UUIDやnil UUIDを流用しません。イベント種別とクラス/対象/結果/理由の許可組合せは、スキーマ版ごとに閉じます。

R04Aは `append_search_audit_on(&mut PgConnection, TypedSearchAuditEvent)` のトランザクションに束縛されたポートと、INSERT専用ロールを提供しますが、業務上の変更を代理実行しません。各イベント生成側は同じ接続・トランザクションで業務行とAudit行をコミットし、Audit INSERT失敗なら変更をロールバックします。拒否時は許可された操作を起こさず、P5の入口で独立した拒否記録トランザクションを試みます。記録失敗でも拒否を維持し、準備完了状態の問題・運用障害として扱います。外部ファイルなどへの作用を、一つのDBコミットと偽りません。破壊的な操作では、先に同一トランザクションで永続的な実行許可行＋Auditをコミットし、その後に外部への作用を実行します。結果も別の同一トランザクションで状態＋Auditに記録します。結果記録の失敗を成功扱いにしません。

| 必須クラス・イベント | 同一トランザクションで生成する所有者 | 対象と失敗時の判定 |
| --- | --- | --- |
| Documentの作成/版追加/公開/取り下げ、ポリシー/ロール、ファイルアクセス | 既存の `document-application` コマンドと `document-repository-postgres` の該当リポジトリトランザクション。`AuditEventRecord`、`targeted_events.rs`、`file_access.rs` などを実DBで照合し、欠落は該当Document所有者へ返す | 既存Documentの型付き対象。Audit INSERT失敗なら変更・原本開示をしない |
| ホスト登録・権限を要するSearch設定変更 | P7-R01Pの `host_inventory_publish.rs` が持つhead CASトランザクション | `CONFIGURATION`/`HostInventory`。登録情報一覧/headとSearch Audit行を同時コミット。コミット結果不明なら再読まで受入を閉じる |
| Audit設定変更 | P7-R04Aの `audit/admin.rs` が持つ版管理されたAuditポリシートランザクション | `CONFIGURATION`/`AuditPolicy`。ファイル/環境変数の直接変更を有効化せず、Audit行とポリシー改訂番号を同時コミット |
| Search再構築・権限を要する管理・破壊的な保守 | P7-R05の `rebuild.rs`/`search_admin.rs` が所有する管理/実行許可/結果のトランザクション。P7-11のGCは既存のガード/現在値/世代の固定境界を維持 | `PRIVILEGED_OPERATION` または `SYSTEM_AUDIT`/`MaintenanceOperation`。Audit失敗ならDB変更をロールバックし、外部作用前の実行許可に失敗したら実行しない |
| 完全性違反 | P7-R05が所有する隔離状態トランザクション（R03は検出して同じポートを呼ぶ） | `SYSTEM_AUDIT`/`SystemComponent`。記録失敗でも安全のための隔離を解除せず、運用障害を明示 |
| 重要な認証・認可失敗 | P5-06の `search-api-http/src/auth.rs` とP5-08の `search-runtime/src/api.rs` にある拒否フックが、R04Aの拒否記録ポートを独立トランザクションで呼ぶ | `SECURITY`/`UnknownTarget`、閉じた理由コード。対象の存在・トークンを含めず、INSERT失敗でも拒否 |

既存Documentの拒否処理 `targeted_events.rs::record_authorization_denied` が使う `AccessPolicy`/nil UUIDは、Document専用の既存表現です。Search/systemイベントの型にはしません。大量に発生する `document.read`、`search.execute`、`search.result.open` の採否は、改訂1の§3どおり規範仕様の担当者とセキュリティ/製品の所有者が先に決めます。採用したクラスは全件記録し、間引きません。R04Aは生成元/保存先のロール・保持・読み取り権限を分離します。Search公開処理/P5/R05の業務ロールはAudit INSERTだけ、Audit配送処理は配送状態列だけを扱い、P6の汎用ロールにはAuditの配送成功確定を認めません。

<a id="4-維持する閉鎖と未了-gate"></a>
## 4. 維持する閉鎖と未達のゲート

改訂1の§4にある `SinkKind × RetentionMode × VisibilityClass` の閉じた許可リスト、全保存先/二つのリース/実ソケット/エクスポーター/保持ハンドルに対する `NoRetention` 検査用標識、§5の隔離したOTLP HTTP/protobuf対gRPCのPoC・厳密な版固定・規範選定前の本番依存関係への追加禁止、§6の変更不能な負荷定義・対象限定の予備測定・予算の事前許可/途中停止・`NOT_ADMITTED` と実測範囲内だけのSLO提案は、**変更しません**。改訂1レビューでこの3件は設計上CLOSEDと判定されたので、実装PASSとは呼びません。

P7-01〜12の同一PostgreSQL上のSource/現在値/READY/世代の固定/ガードと、P6のSearchイベント処理記録/汎用配送成功確定、P1のDocument/Unit/字句検索、P3の条件付きGraph、P4のRemote RAM、P5の四つのルート/二つのリースは、そのままです。P3-P04が未採択なら、Graph/P7のREADY・公開、R02/R09の本番受入を閉じます。P2 Vectorが `Disabled` でも、中立なCore・字句検索・Graph/現在値/最終ゲートを省きません。P1〜P6の最終検証記録を集約するのは、R09だけです。

当時の次の作業は、[改訂計画2](p7-runtime-plan-revision-2.md)と本書を、独立したアーキテクチャ・セキュリティ審査担当が再判定することです。ホスト公開処理/型付きAudit生成元と各イベント生成側の実PostgreSQL別接続・再起動・ロールバック、P5拒否フック、実ロール、P3の適格性検証、P5ソケット、P6の結果不明なCOMMIT、P7-12の別DBへの復元、OTLP採択、処理容量/SLOは、未実施の実装・適格性検証ゲートです。設計だけで本番稼働の準備完了を宣言しません。

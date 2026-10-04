<a id="p7-production-runtime--designplan-freeze"></a>
# P7 本番ランタイムの設計・計画凍結記録

[翻訳元の固定原文（commit 0ecf486719e3c9d71242e289a7564ad6d1032b3c）](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-freeze.md)

本書は意味保存の日本語訳であり、原設計の再承認・資格追加ではありません。既存承認hashは当時の原文・証拠を指し、訳文hashではありません。以下の状態・次の作業は当時の記録で、現在の実行指示ではありません。本文中の元の行番号も当時の原文を参照しており、訳文の行番号ではありません。旧見出しへのリンクは明示的なIDで維持しています。

状態：**FROZEN — design/plan only（設計・計画だけを凍結）**（2026-10-01）。独立したアーキテクチャ・セキュリティの[改訂2レビュー](p7-runtime-architecture-review-revision-2.md)は、後述の厳密に特定した入力に対して、**PASS / GO — 設計・計画の凍結に限る**と判定しました。本書は、その統合した設計を固定します。本番コード、実行時の動作、P1〜P6の最終受入、P3のバックエンド採択、P2のVector採択、本番稼働の準備完了、SLO保証、マージ、本番データベース移行、本番へのデプロイについて、資格取得や実施を示すものではありません。

<a id="1-authority-と-exact-provenance"></a>
## 1. 規範の正本と入力の厳密な来歴

`spec/`が規範の正本です。[P7共有永続化基盤の凍結記録](p7-shared-durable-freeze.md)と、P1〜P6それぞれの既存の凍結内容・所有者の責務境界を維持します。以下は、`feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff`の未コミット作業ツリーでSHA-256を再照合した、今回の直接入力です。ハッシュに対応するリンクは当時の原文へ固定し、現在の日本語版を別に案内しています。

| 直接入力の固定原文 | SHA-256 | 役割 | 現在の日本語版 |
| --- | --- | --- | --- |
| [p7-runtime-design-revision-2.md](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-design-revision-2.md) | `415d5a1648702670eb5737eecfe16581b7565191f2e64409c143236a00e95461` | ホスト登録情報の公開処理と、型付きAuditイベントの生成処理に関する最終設計差分 | [設計改訂2](p7-runtime-design-revision-2.md) |
| [p7-runtime-plan-revision-2.md](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-plan-revision-2.md) | `336894fcfaba7adf2e67474b56357ddfa7e9ad4502e1a0206616cb4743d9cb6b` | 書き込み担当の一元化、作業順序、異常系試験に関する最終計画差分 | [計画改訂2](p7-runtime-plan-revision-2.md) |
| [p7-runtime-architecture-review-revision-2.md](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-architecture-review-revision-2.md) | `1070a12609b408c655b03ef0da4e20b819093dca7befb8691941953b36a46172` | 独立した設計・計画のGO判定とW1/W2 | [独立レビュー改訂2](p7-runtime-architecture-review-revision-2.md) |

統合元の内容の同一性も固定します。

- [元設計の固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-design-completion.md)：`df0f2ba4e91526a24567addfac3777a7d7219ae296d5a3dc4b0dcc0c51737f3e`（[日本語版](p7-runtime-design-completion.md)）
- [元計画の固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-plan-proposal.md)：`acecf896ad5481955b805728bb8c0d970e154657e1a4d5a89f7c100f3bc0ed3a`（[日本語版](p7-runtime-plan-proposal.md)）
- [初回NO-GOレビューの固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-architecture-review.md)：`0bf9ce336e664a3eb4a5e14c7f37ec9a351bc4edbfa9d6b8b6435f002088f793`（[日本語版](p7-runtime-architecture-review.md)）
- [設計改訂1の固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-design-revision-1.md)：`422e00dcb0c685feb2fc9f81755e25bdd555d2e4b7602ccadb85e4c281a9d035`（[日本語版](p7-runtime-design-revision-1.md)）
- [計画改訂1の固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-plan-revision-1.md)：`d24cba464e40f9d07d2baf9095d4ece1bb099f0f8b06cfaca6eee2e2982e3d57`（[日本語版](p7-runtime-plan-revision-1.md)）
- [改訂1レビューの固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-architecture-review-revision-1.md)：`a6f91b67ecfd220191486aa2cdd6715967a8115f9e0b9dc6aee8ecaee6c19e29`（[日本語版](p7-runtime-architecture-review-revision-1.md)）
- 入力として用いる[共有基盤の凍結記録の固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-shared-durable-freeze.md)：`20b5b64ac6c8e6209a3618e1c8f4577f1f48333991df0cffe9af96e2cbd5e110`（[日本語版](p7-shared-durable-freeze.md)）
- 入力として用いる[共有基盤の計画の固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-shared-durable-plan.md)：`97c122bf4447dd63caeac24930503247c11d52e533e1825d913f737162812517`（[日本語版](p7-shared-durable-plan.md)）

同じ対象について内容が抵触する場合は、改訂2 → 改訂1 → 元設計・元計画の順に適用します。改訂2は残っていた二つの指摘への差分であり、改訂1や元文書の抵触しない条件を取り消しません。[範囲を限定した実装計画](p7-runtime-plan.md)に、この凍結内容の実行順と受入条件を記載しています。

<a id="2-frozen-runtime-contract"></a>
## 2. 凍結したランタイムの契約

本文中の「Searchイベント処理記録（Search receipt）」はデータベース上のイベント処理記録を指します。各工程の「検証記録・証拠（receipt）」とは区別します。

1. **構成を一つにまとめ、既存の所有者を維持する。** 一つのRustランタイムの構成起点が、P7-01〜12の同一PostgreSQL上のSource、current（現在の世代）、READY（準備完了状態）、pin（世代の固定）、guard（構築中の保護）とSearchイベント処理記録、P1のDocument/Unit/字句検索、条件付きのP3 Graph、P4のRemote RAM評価、P5の四つのルートと二つのリース、P6の汎用配送処理を接続します。P7-Rxxは受入済みの生成側処理を利用し、第二のSourceポインター、Source/actorの発行処理、ルートハンドラー、汎用Domain確認応答、Searchイベント処理記録、Graph READYを再実装しません。P1〜P6の最終検証記録を集約するのはR09だけです。
2. **ホストの正本と公開処理。** R01の信頼された運用者入力`HostRegistrationInputV1`を、唯一の登録情報作成用入力とします。これは、登録集合とは独立した全テナントの一覧（登録0件のテナントを含む）、Document/Remoteの全DTO、エポック、単調増加する改訂番号、宣言件数を提供します。提供できない場合は、安全側に倒して処理を拒否します。R01PだけがSearch `0004_host_registration_inventory_v1.sql`の独立した登録情報一覧と現在のheadを公開します。変更不能な全行、テナント・名前空間のダイジェスト、旧headを条件とするCAS（比較交換）、`host.registration.changed`の型付きAudit行を、一つのPostgreSQLトランザクションで確定します。不完全な入力、衝突、古い状態、Audit INSERT失敗、コミット結果不明の場合は、公開と処理権の取得（claim）を停止し、別接続で再読して確定します。認証情報の値は登録情報一覧に保存しません。
3. **読み取り側と起動許可。** R02は公開処理に対する読み取り専用アダプターです。同じ読み取りトランザクションでheadと行を読み、ホスト側の作成時の改訂番号・テナント一覧と別接続で照合します。Document/Remoteの`CompleteDesiredRegistrations::capture(...).await`を同一の正本にまとめ、P7-02の唯一の`PgPool`台帳と`SourceRegistrationCatalog::try_new(...).await`を使います。ホストのhead、台帳・所有者・種別・有効化状態・current、P7-12の走査の再照合が終わるまで、P5リスナーとP6のclaimを有効にしません。再読み込みは、全検証後の一回の切替に限ります。
4. **型付きAuditとトランザクションの所有者。** 既存のDocument `audit_outbox_events`のUUID・対象に関する制約を保ち、Domain `0010_audit_delivery_v0.sql`に、別の追記専用`search_audit_outbox_events`と版管理されたポリシーを追加します。R04A-Sの`append_search_audit_on(&mut PgConnection, TypedSearchAuditEvent)`は、R01PのホストCAS、R04A-Cのポリシー操作、R05の管理・実行許可・結果記録・隔離、P5の拒否、Documentの各既存所有者が、それぞれ自分のトランザクション境界で呼び出します。Search/systemの対象をDocumentの`AccessPolicy`やnil UUIDに偽装しません。Audit INSERT失敗時は対応する変更をロールバックし、拒否記録に失敗した場合も拒否を維持します。外部への作用は、永続的な実行許可とAuditのコミットが完了した後だけに行い、結果の記録失敗を成功として扱いません。R04A-DはDocument/Search両方のAudit生成元から、生成元タグ付きデコーダーと、別のロール・フェンス・再試行・DLQを使って、別PostgreSQLの配送先へ送ります。P6 Domainの`delivered_at`、Searchイベント処理記録、各Audit確認応答は互いに独立しています。
5. **保持、テレメトリー、測定。** 改訂1で設計上CLOSEDとなった三件を維持します。R04Oは、`SinkKind × RetentionMode × VisibilityClass`の許可対象を閉じた一覧にし、既定で拒否します。P4/P5の二つのリース、実ソケットでの送出、全配送先・エクスポーター・保持ハンドルの`NoRetention`検査用標識を要求します。R04PはHTTP/protobufとgRPCを隔離PoCで比較し、独立した採択と規範上の厳密な版固定が済むまで、`POC REQUIRED`の通信方式を本番CargoやR06イメージへ入れません。R07は変更不能な負荷定義、範囲を限定した予備測定、資源の事前許可・途中停止、未測定を表す`NOT_ADMITTED`を要求します。R08は、対応する実測範囲に限った運用SLOの*提案*を作ります。
6. **資格判定の境界。** P3-P04のバックエンドは未採択であり、Graph/P7のREADY・公開と、R02/R09の本番受入は閉じたままです。PostgreSQL以外のGraphが採択された場合は、同等の公開・フェンス機構について別設計と独立レビューが先に必要です。P2のVector・ランタイム選定も、実測と独立判定まで閉じたままです。`Disabled`を選ぶ場合でも、中立なCore、字句検索、Graph/current/最終アクセス確認のゲートは省きません。ホストの識別、実ソケット、実ロール、P6の結果不明なCOMMITとフェンス付き確認応答、P7-12の別DBへの復元、OTLPの採択、処理容量・SLOは、それぞれの実装・資格判定の検証記録で別々に証明します。

<a id="3-review-closure-と実装時-gate"></a>
## 3. レビュー指摘の解決と実装時の必須条件

初回レビューの五件のうち、配送先別の`NoRetention`、OTLP通信方式のPoC、負荷・予算の三件は、改訂1レビューで**設計上CLOSED**となりました。ホスト登録情報一覧の公開処理と、型付きSearch/system Auditの生成処理の二件は、改訂2レビューで**設計上CLOSED**となりました。これは、指定された試験、実ロール、ランタイム、実測がPASSしたことを意味しません。

- **W1（実装受入）：** `spec/operations/observability-audit-requirements-v0.md` §14.2/§15に従い、必須の`document.version.read_confirmed`を、Document生成側のクラス表とR04A-Sの既存形式用デコーダー・ポリシーに明記します。Documentの所有者が、既存の`crates/document-repository-postgres/src/read_state.rs`にある同一トランザクションのINSERTを実DBで照合し、このクラスに固有のAudit INSERT失敗によって、初回の既読状態もロールバックされる異常系試験を実施します。R04Aの配送試験だけで、生成側の実装が完了したとは数えません。
- **W2（新規DBの初期構築受入）：** Domain `0009` → Search `0001`〜`0003` → Domain `0010` → Search `0004`の段階順、両移行台帳のチェックサム、実接続のロール・権限付与のゲートを、全移行ファイルが存在する新規の使い捨てDBでR03/R09が検証します。現行の`search-runtime::migrate`にある単一の`sqlx::migrate!("./migrations")`だけを、順序の証拠にしません。欠落、逆順、チェックサム不一致、過大な権限がある場合は、リスナー・claim・準備完了状態を有効にしません。

W1/W2は、この設計・計画凍結の阻害要因ではなく、実装と新規DBの初期構築における未達のゲートです。当時の次の具体的な作業は、[計画](p7-runtime-plan.md)の依存順に、R04A-SとP7-03の受入済みスキーマ・ロールを満たし、R01Pの実公開処理、R02の実アダプター、各生成側処理、R03/R09の新規DB初期構築を、対象を絞ったRED→GREENと独立した読み取り専用レビューで証明することでした。Graph/Vectorの選定とP1〜P6の最終検証記録を先取りせず、R09の最終ゲートに合流させます。

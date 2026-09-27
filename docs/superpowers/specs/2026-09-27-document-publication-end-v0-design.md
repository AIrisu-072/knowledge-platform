# Document Publication End v0 — 設計

- 状態: **提案中 — 日本語版の確認待ち**
- 日付: 2026-09-27
- 対象: `Document Publication End v0`（transaction T10）
- 作業区分: Product Capability / Architectural
- 基準: `feat/document-versioning-v0@96b068bc9484a219b435d0633ea6669ddb7d7f97`（PR #12。未マージの PR #11 を基点とする）

## 1. 目的と承認された方針

文書全体の公開を、内容の削除や個別 Version の取下げを行わずに終了する。これは `spec/data/transaction-consistency-requirements-v0.md` の T10 で要求された独立操作である。T4 の Version 取下げでは直前の公開版が現行版へ戻り得るため、T4 だけでは文書全体を通常利用・通常検索から確実に外せない。

依頼者が 2026-09-27 に承認した方針は、現行版参照を null にし、過去の Version と原本を保持し、冪等な公開終了操作を記録し、公開予約を無効化し、検索除外イベントを発行し、通常の読み取りを現行版に限定する、というもの。再公開は後続の別操作として設計する。この文書は書面での確認が完了するまで提案であり、会話中の方針承認だけで本番実装を許可しない。

## 2. 規範仕様と対象範囲

`spec/data/logical-data-model-v0.md` と `spec/data/transaction-consistency-requirements-v0.md` を規範仕様とする。承認済みの Document Versioning v0 設計・実装が直近の基準である。Audit の生成は `spec/operations/observability-audit-requirements-v0.md` に従う。Search Index は transaction 要件に従い、正本より遅延してよい。T10 の未確定部分は、この設計の承認後、本番実装前に `spec/` と整合させる。凍結済みの Versioning 規則との衝突が見つかった場合は、暗黙に変更せず改訂承認を受ける。この提案は取下げ時の旧版復帰、Version の同一性、DSI、予約公開の意味を変更しない。

対象は、現行公開版を持つ Document の公開終了、その transaction と完全な再実行、公開予約の無効化、通常読み取りの可視性、Search 除外のための正本側証跡である。HTTP/UI、Search への配送・Index 実装、AccessPolicy 実装、法的削除、自動再公開、過去 Version の内容・状態の変更は対象外とする。Search Extraction と DSI は引き続き別経路とし、公開終了の実行にはどちらも必要としない。

## 3. 状態と意味

Document の論理的な同一性は維持する。T10 は、現行の `PUBLISHED` Version を指す `Document.current_version_id` を null にし、`Document.revision` を 1 増やす。元の現行 Version は `PUBLISHED` のままとし、その `published_at`、内容、base 関係、原本は変更しない。T10 のために `DocumentVersion` を `WITHDRAWN` にせず、Document の新しいフラグ、Version の lifecycle state、重複する `ended` 列も追加しない。

現行版参照が null という事実だけでは、未公開、旧版へ戻せなかった取下げ、文書全体の公開終了を区別できない。そのため、追記型の永続的な T10 操作記録を「文書全体の公開を終了した」証跡とする。Document 単位の「公開終了」表示は、その記録と null の現行版参照から導出できる。Version 単位の表示は引き続き lifecycle と現行版参照に従い、元の現行 `PUBLISHED` Version は過去版となる。通常の読み取りから現行版は見えなくなる。権限に基づく過去資料へのアクセスは別経路とし、通常読み取りのフォールバックには使わない。

T10 v0 は通常の公開操作に対して終端となる。既存の `WORKING` Version と過去版は保持するが、Version の作成・更新・rebase、予約・期限到達・手動 Publish、その他の現行版を設定し得る操作は、T10 記録がある Document に対して拒否する。将来の再公開には、独立した監査対象 transaction と可視性の規則が必要である。権限がある場合、T10 後も過去版に対する T4 取下げは許容する。ただし現行版参照は null のままとし、文書を再公開しない。

## 4. コマンドと再実行

`EndDocumentPublication` には、呼出側が生成する UUIDv7 の操作 ID、Document ID、期待する Document revision、期待する現行 Version ID、実行者 `PrincipalRef`、空白のみではない理由を含める。期待する現行 Version ID により、並行した切替後に別の版を終了してしまうことを防ぐ。コマンドの同一性は呼出側が指定した全フィールドから決定し、その決定的な digest を保存する。結果には操作 ID、Document ID、元の現行 Version ID、null となった現行版参照、更新後 revision、UTC の `ended_at` を含める。

専用の `document_publication_end_operations` 台帳では、操作 ID を主キー、Document ID を v0 の一度限りの終端操作に対する一意キーとする。コマンド digest、期待する現行版と revision、実行者、理由、元の現行版、結果、日時を保存し、参照には同一 Document の外部キーを使う。同じ ID・同じコマンドの再実行は、現在の null 参照や古くなった期待 revision を判定する前に保存済みの結果を返し、revision やイベントを追加しない。同じ ID で異なるコマンドなら Conflict、完了済み Document に別 ID で要求した場合は BusinessRule/AlreadyEnded として変更しない。commit 結果が不明な場合は同じ操作 ID で照会・再試行し、新しい ID を作って推測しない。

新規 T10 要求には、現行 Version が存在し、同じ Document に属し、`PUBLISHED` であることを要求する。現行版参照が null の場合は、未公開または T4 取下げ後を含めて業務上の拒否とする。これは「現在公開中の文書を終了する」という承認済みの範囲に一致する。Document がない場合は NotFound、期待 revision または現行版が古い場合は Conflict とする。AccessPolicy と transport での認可は Versioning v0 と同様に本機能の対象外とする。公開 T10 transport は追加せず、将来 transport を追加する場合は、この内部 Application コマンドを呼ぶ前に実行者を認可する。

## 5. 原子的な transaction と競合

Application は完全な再実行を先に解決し、コマンド形式と信頼済み実行者参照を確認してから、専用の Repository 操作を呼ぶ。Repository はまず Document 行をロックし、T10 台帳、期待 revision・現行版、現行 Version が同じ Document の `PUBLISHED` 版であることを再確認する。そのうえで、以下を行う。

1. `current_version_id = null` とし、Document revision を 1 増やす。
2. その Document のすべての `PENDING` 公開予約を、理由 `document_publication_ended` の終端状態にする。予約履歴を残し、対象 Version の `scheduled_publish_at` 投影を消す。
3. T10 の操作結果と、Domain Outbox イベント 1 件、必須の Audit Outbox イベント 1 件を記録する。

これらを一つの transaction で commit する。イベント種別は `DocumentPublicationEnded` と `document.publication.ended` とする。payload には Document、元の現行 Version、null の結果、更新後 revision、操作 ID、実行者、理由、UTC の終了日時、無効化した予約件数を含め、文書本文や保管先の資格情報は含めない。Domain イベントを受けた Search 側は、その Document の通常検索エントリをすべて除外する。少なくとも 1 回の配送では、イベント ID と Document revision によって冪等に処理できるようにする。Audit の配送は遅延してよいが、Audit Outbox レコードの生成に失敗したら T10 を commit しない。

Document 行ロックと revision の照合により、T10 は T3 Publish、T4 取下げ、公開予約の登録・取消・期限到達、Version 変更と直列化される。他の操作が先に成立した場合、T10 は Conflict とし、呼出側に再読込を求める。T10 が先に成立した場合、後続の期限到達 worker は終端済み予約を見て Publish できない。現行版を新たに設定し得る Version 作成・公開経路、現行版が null の場合の手動初版 Publish、公開予約の登録経路は、自身のロック済み transaction 内で T10 記録を確認しなければならない。事前検査だけでは足りない。T10 は公開範囲を狭め、内容を復帰させないため、Storage または DSI の障害で妨げない。

## 6. 通常読み取りと Search の整合性

既存の PostgreSQL `load_current` は `COALESCE(current_version_id, latest WORKING, latest Version)` を使う。現行の `DocumentService::get_document` と `open_primary_file` もその集約を使うため、参照を null にするだけでは旧版ファイルが見えてしまう。T10 では、`documents.current_version_id` と同じ Document の `PUBLISHED` Version だけを結合し、フォールバックしない公開中の現行版専用クエリを設ける。現行版参照が null なら通常読み取りの結果はない。null でない参照が非 `PUBLISHED` または別 Document の Version を指す場合は、フォールバックせず整合性違反とする。

通常の Document 取得とファイルを開く API は、この現行版専用クエリを使う。既存のフォールバック取得を残す場合は、内部の編集・操作用 snapshot と分かる名前に限定し、通常・公開読み取りへ流用しない。既存の下書き取得呼出側は、通常読み取りの可視性を引き継がせず、明示的に別の編集用経路へ移す。T10 は公開の過去資料閲覧 API を追加しない。将来の過去版閲覧には、Version ID の指定と AccessPolicy による認可を必要とする。

Search からの除外は非同期である。検索結果を通常利用者や LLM に表示する前、またはファイル・内容を提供する前に、Document 側で結果の Document ID・Version ID が現在の `PUBLISHED` 版と一致するか検証する。T10 後の古い検索結果は、Index の削除が遅れていても抑止する。Search Index の再構築元は現行 `PUBLISHED` Version だけを列挙し、過去の `PUBLISHED` 行から T10 文書を再登場させない。本設計が定めるのは Document 側の契約であり、Search consumer の配送実装は別機能とする。

## 7. 必要な受入証拠と実装ゲート

後続の実装計画では、以下について絞った RED/GREEN 証拠を要求する。

1. 公開中の Document に T10 を実行すると現行版参照が null、revision が 1 増加となる。元の現行版は `PUBLISHED` の過去版として残り、元の `published_at` と FileObject を含めて変更されない。
2. 現行版なし、別 Document の版、非公開状態の版、古い期待現行版・revision、および異なる T10 ID の並行実行から、誤った公開終了記録が生まれない。
3. 同じ ID の完全な再実行、同じ ID・異なるコマンドの Conflict、commit 結果不明時の復旧で、台帳 1 行と Domain/Audit イベント各 1 件になる。
4. 初版・後続版の予約は投影を消して終端となり、重複した期限到達 worker や手動の初版・後続版 Publish は T10 後の現行版を設定できない。
5. 通常の Document・ファイル読み取りと古い Search 結果の検証は終了済み内容を隠し、内部・過去版 snapshot は定義された別経路だけに残る。
6. 過去版に対する T4 取下げは別操作のままである。文書全体の公開終了のために Version を `WITHDRAWN` にせず、Storage・DSI の事前検査が T10 を妨げない。
7. 除外イベント、Search 再構築用の現行版専用の正本クエリ契約、Audit の原子性、原本・過去記録の保持が、規範仕様 T10 の境界を満たす。Search consumer・再構築の実装は対象外とする。

実装中は対象を絞ったローカル確認を行い、まとまった head に対してホスト CI を 1 回確認する。小さな Task ごとに 3 種の CI を実行しない。書面仕様の確認後に、別途レビューを受ける実装計画を作り、それまでは本番コードを変更しない。PR #11 と PR #12 は未マージであり、T10 の設計・実装によってマージが許可されるわけではない。

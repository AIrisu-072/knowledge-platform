# Document Publication End v0 — 設計改訂案 1：読み取り境界

- 状態: **承認済み — T10 設計 §6 と受入項目 5 に反映**
- 承認日: 2026-09-27 JST
- 承認記録: `2026-09-27-document-publication-end-v0-design-amendment-1-approval.md`
- 対象: `2026-09-27-document-publication-end-v0-design.md` の §6「通常読み取りと Search の整合性」および受入項目 5
- 変更しない範囲: T10 の公開終了 transaction、状態・操作台帳、旧版保持、予約無効化、Outbox、Search 除外、再公開の別設計

## 発見した衝突

凍結済みの `2026-09-16-document-authoritative-core-design.md` は、`CreateDocument` で `WORKING` 初版を作成した後、`GetDocument` と原本読込でその版を取得する契約を §§3.1、13、18.5 に定めている。既存テストもこの経路を確認している。

承認済み T10 設計の §6 は、既存の `DocumentService::get_document` と `open_primary_file` 自体を現行 `PUBLISHED` 版専用に変えるよう読める。このまま実装すると、公開前 `WORKING` 版への `GetDocument` が失敗し、Authoritative Core の凍結済み契約を暗黙に変更してしまう。

## 提案する限定改訂

1. 既存の `DocumentService::get_document` と `open_primary_file` は、未終了 Document の編集・authoritative 読み取りとして維持する。初版 `WORKING` の Create→Get→open は引き続き成功する。ただし T10 が commit 済みの Document には、既存のフォールバックを適用せず NotFound とする。T10 台帳の確認と取得は同じ DB statement で行い、終了後に開始した読み取りから旧版を見せない。
2. 通常公開用に `get_current_published_document` と `open_current_primary_file` を別に設ける。これらは `current_version_id` が指す同一 Document の `PUBLISHED` 版だけを返し、`WORKING`、過去版、T10 終了済み Document を返さない。将来の通常利用・Search 連携 transport はこの経路を使う。
3. Versioning の内部操作と履歴照会には、明示的な Version ID の snapshot または内部専用 port を使う。T10 後の許可された過去版 T4 と、Create の commit 結果照会はこの内部境界に残す。通常公開 API のフォールバックには接続しない。公開の過去版閲覧は引き続き AccessPolicy を要する後続機能とする。
4. 検索結果の現行性照合は承認済み T10 設計のまま維持する。公開終了後の旧版 hit は、Index の更新を待たずに抑止する。

## 受入条件と影響

- 既存の Authoritative Core Create→`GetDocument`→原本読込のテストは、未終了の `WORKING` 版で引き続き成功する。
- T10 後は既存の `get_document`・`open_primary_file` でも旧版を開けない。
- 新しい通常公開用 API は、公開前、T10 後、過去版を返さず、現行 `PUBLISHED` 版のみ返す。
- 内部の Versioning 操作・履歴 snapshot は T10 後も必要な証跡を扱えるが、通常公開用 API には流れない。
- Document Authoritative Core 自体の凍結設計は変更しない。T10 の読み取り API の割当てだけを改訂する。

## ゲート

依頼者は本改訂を「承認します。」と回答した。T10 設計 §6 と実装計画の Task 4 に反映する。実装計画全体の承認は別のゲートであり、その承認前に本番コードへ進まない。

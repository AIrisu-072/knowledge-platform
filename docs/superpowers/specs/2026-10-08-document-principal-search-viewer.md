# 文書アクセスへの新主体追加・原本ビューア：設計追補案

- 状態: PROPOSED / 設計判断待ち。実装・設計凍結・本番Identity接続の承認ではない。
- 調査基点: `main@2a37d35cd228344f98e0194de16d5336fa786e3c`（2026-10-08 UTC）。
- 対象: 残タスク8（利用者・グループ検索から文書ACL追加）、9（原本ビューア）。製品source・実アカウント・実権限は変更していない。

## 既存契約と調査結果

正本は `spec/api/openapi.yaml`、`spec/api/schemas/document/models.yaml` と承認済みDocument Management Basics v0 §5、§12。主体キーは `(identityProvider, subjectKind, subjectId)`。表示名で同一視しない。5権限は独立しており、アクセス管理だけで内容閲覧は許可されない。明示policyは全体置換であり、親と子の権限を合算しない。

`DocumentAccessPolicy.tsx` は読み取った `effectiveGrants` の権限変更・削除と継承切替を提供する。新主体追加入口はなく、再確認時の `adopt(..., preserve=true)` は最新effectiveGrantsにある主体だけを残す。そのため候補を入力欄へ足すだけでは再確認で新主体が失われる。既存の固定operationId/body、OCC、UNKNOWN回復、自己失権後の結果保持を維持した別のdraft reconciliationが必要になる。

Document OpenAPIに主体directory検索endpointはない。既存 `PolicyGrantInput` はprincipal/group/roleのprovider付き参照を受けるが、このschemaは主体実在・有効性の保証ではない。Organizationの `organization-policy.ts` / `work-api.ts` は固定のSyntheticPrincipal一覧を表示するPoCであり、実組織の利用者・グループdirectoryではない。Organizationの職務roleをDocumentのIdentity role/groupへ無条件に変換しない。

`listVersionFiles` は各原本のcontentItemId、representationId、logicalPath、ordinal、role、displayName、mediaType、sizeBytesを返す。`downloadVersionFile` は同じDoc/Version/原本をpurpose付きで認可し、Audit commit後にbytesを返す。既存purposeはpublished/authoring/history。Rangeはv0で416。`BinaryTransportBridge.downloadVersionFileBlob` はAbortSignalと120秒deadlineに対応するが、viewer用byte上限はない。

`OriginalVersionDownload.tsx` は最初の原本をダウンロードする共通入口。詳細内のDownloadButton、履歴、根拠、Organization文書contextにも別取得入口がある。既存downloadをviewerへ一括置換すると履歴の認可、古い応答、画面離脱の扱いまで影響するため、まず詳細の原本一覧に選択した1件の「表示」を追加する案とする。

既読は [承認済み詳細表示設計](2026-10-07-document-view-read-state-design.md) のまま、利用者×現行公開版の詳細正常表示時に記録する。原本表示・何ページ読んだか・同意・読了を表さない。viewerへの入口追加で既読送信を増やさず、history/authoringのviewerで現行公開版を既読にしない。

## 提案A：新主体検索と追加

1. Documentの既存manageAccessが許可された対象に限定し、検索を許可するread-only directory portを設ける。候補はprovider/kind/id、表示名、有効状態を持つ。初期対象はprincipal/groupのみ。role追加は別判断とする。
2. API候補は `GET /v1/documents/{documentId}/access-policy/subjects`、query/kind/pageSize/cursor付き。これは未承認の新endpoint案であり、既存APIに存在すると扱わない。検索範囲・表示項目は認証済みIdentity adapterが制限する。メール等の不要な個人情報を返さない。外部directory取得中にDocument transaction lockを保持しない。
3. 合成directory fixtureで検索とACL追加機能を検証する。本番directory provider、接続資格、利用者作成、group所属更新は運用承認後の別作業。秘密情報をチャットで受け取らない。
4. 候補選択はdraftへの追加だけ。初期actionsは空、保存には少なくとも1権限を明示選択する。既存行との重複は完全キーで防ぎ、同じ表示名・異provider/kindの行を区別する。検索候補の選択だけで権限を付与しない。
5. 保存直前に最新Document/ACLと候補の有効性を再確認する。新主体の可用性を確認できない場合はfail closed。外部directory変更とACL transactionは原子的にできないため、候補検証の有効期間と再確認契約を明示する。既存の不明主体行を新主体追加時に黙って削除しない。
6. 再確認時は既存行変更・削除と新主体追加を分離して保持し、backend最新状態との差を表示して再承認する。UNKNOWN中は検索・draft変更・別保存を封鎖し、元operationId/bodyのみ再送する。

### 実装ファイルと依存

OpenAPI/models → directory port/application → HTTP handler/config/fixture → generated client → draft reconciliation/helper → DocumentAccessPolicy → DOM/runtime検証 → 日本語操作・運用文書の順。Folderへの新主体追加、Organization policy画面変更、実directory同期は今回案の対象外。DocumentAccessPolicyと既存ACL回復storeを変更する担当は一人に固定し、viewerは別componentにして衝突を避ける。

## 提案B：限定原本ビューア

最小案は詳細の固定Version原本一覧からtext/plain 1件を表示するviewer。取得は既存binary API、本文はReactのtext nodeで表示し、HTML/XML/SVGを実行・iframe表示しない。既存ダウンロードは保持する。PDFは次の選択を設計判断に含める。

- PDFも初回対象にする場合はブラウザー標準PDF表示またはPDF renderer依存のどちらかを選ぶ。標準表示は対応ブラウザー・CSP/Blob frame・PDF内active content・印刷/保存・取得済みbytesの回収限界を資格付けする。rendererはdependency/security/license/容量・描画試験を追加する。現在どちらも承認・実装していない。
- DOCX/XLSX/PPTXは原本ダウンロードの案内を表示し、無断で外部変換サービスへ送信しない。任意MIMEのinline renderingは導入しない。
- 容量案は1原本10 MiBを初期上限とするが未承認。manifestだけでなく実受信bytesも上限判定し、上限超過は表示を拒否して通常downloadへ案内する。既存Blob全量取得だけでは受信中のメモリー上限を保証できないため、streamのbounded read/cancel/AbortSignal対応を先行する必要がある。

固定対象はDoc/Version/purpose/contentItemId/representationId。表示前後でmanifest・通常read・capabilityの有効性を検査し、別原本選択・版切替・route離脱・非表示・query失効・401/403/404時はabortして表示を破棄する。古い応答からobject URLや本文を作らない。閉じる/unmountでobject URLをrevokeする。表示済みbytesを利用者端末から完全消去できるとは保証しない。バックグラウンドで自動取得しない。

原本表示は既存download監査を使い、「読了」監査・既読trigger変更は追加しない。PDF表示に成功しただけで全ページを読んだことにしない。将来、原本表示時への既読変更を望む場合は、対象representation、複数原本の何件／全件、viewer成功の定義、新公開版・原本追加時のreset、複数端末／非表示を別設計で確認する。

## 受入試験と操作文書

主体: 合成principal/group、同名異provider、重複、検索0件/失敗、空権限、追加と削除の混在、継承→明示、同tick二重送信、検索古い応答、権限喪失、再確認で追加案保持、OCC、UNKNOWN固定再送、一覧往復、reload後の未保存案消失を確認する。実アカウントへの権限付与を試験としない。

viewer: 複数原本選択、textのDOCTYPE/HTML文字列の非実行、非UTF-8/空file、MIME偽装、容量境界/超過、通信途中失敗、同tick切替、古い応答、非表示、公開版切替、history認可拒否、原本権限喪失、route離脱、再起動後の同じ固定原本取得を確認する。API監査の成否とviewer表示の成否を分ける。既読関連の既存試験とsnapshotを維持する。

操作文書には「検索→選択→権限を選ぶ→変更理由→最新設定確認→保存→結果確認」と「原本を選ぶ→表示→閉じる／ダウンロード」を記載する。検索候補が利用者作成・group所属変更ではないこと、UNKNOWN時の同じ操作再送、既読が原本読了を示さないことを明記する。

## 親へ返す重要な設計判断

1. 主体検索を合成directoryで先に完成させ、本番provider接続を運用承認まで保留する案でよいか。検索許可範囲は対象DocumentのmanageAccessに限定する案。
2. viewer初回対応をtext/plainだけにするか、PDFも必須とするか。PDFが必須なら表示方式の選定が必要。
3. viewer表示上限10 MiB案と、超過原本はdownload案内にする案。

既読の現行detail表示triggerは保持するため確認を繰り返さない。将来の原本表示trigger／複数原本既読条件は未決定として保持する。

## 今回の検証と残り

実施: main基点・source/API/specの静的照合、設計追補案作成。製品実装、runtime試験、本番Identity接続、実権限変更、対象PC導入は未実施。`.agents/skills` はこのcheckoutに存在せず、repository内のSKILL.mdも見つからなかった。設計承認後のexact actionはdirectory契約とviewer方式の確定→実装計画→合成RED契約であり、この文書のみで製品完成とはしない。

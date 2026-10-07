# 文書詳細表示による既読・未読戻し：再実装の限定要件

基点main: a7cf93d53a1b1627ace31d079fd222d7400d8673 / tree8f2834fbc9f46f169a5069778ec56b191ba9e09e。2026-10-07の承認済み意味を記録する。旧未公開sourceは実行環境の喪失後に取得できず、この文書はそのbytesの復元や旧試験資格の継承ではない。

## 承認と範囲

依頼者は「文書の詳細を正常に表示したら既読」「未読に戻すで再確認対象」「過去の閲覧記録は保持し読了証明に使わない」という確認に対し、「その意味での既読未読に変更していいです」と承認した。文書単位の表示、内部の本人×公開版、新版公開後の未読を維持する。原本viewer、複数原本の読了判定、他人の既読管理は追加しない。

Tauri、Organization、Audit配送、Search製品は他担当のまま。main.tsxはDocument専用監視のimport/installだけとし、Runtime/OrganizationProviderの順序・lifetime・router treeを保持する。旧明示ボタン試作や失われた実装の成功件数は新sourceの証拠にしない。

## 保存と互換性

- 自然キーはidentity_provider×principal_id×document_version_id。既存first_read_atは不変
- document_read_statesへneeds_recheck BOOLEAN NOT NULL DEFAULT FALSE、read_state_revision BIGINT NOT NULL DEFAULT 1を追加。revisionは1〜9007199254740991。行なしはfirstReadAt:null/needsRecheck:false/revision0/isRead:false
- 既存rowはr1/falseへ移行し、初回日時・旧Auditを変更しない。isReadは初回日時ありかつ再確認false、unreadOnlyは行なしまたは再確認true。一覧/詳細の既存wire形は変えず、その意味を同じprojectionへ揃える。VersionのfirstReadAtは履歴であり現在badgeの根拠にしない
- 旧空body PUT markDocumentVersionReadと4応答field(documentId/versionId/firstReadAt/inserted)は維持。既存rowの再生はresetを解除せずrevisionも増やさない。旧PUTで最初のrowを作る場合だけdefault r1となる
- 新migration0012_document_current_read_state.sql。既存migration本文/checksumは不変。既存Document台帳集合の3assertion(search_main_migration/source_ownership_migration/coordination_migration)だけ1..=11→1..=12へ追従し、集合検査を弱めない。直前mainで番号衝突を再確認する。Search専用の追加実行はしない
- 新旧serverの混在稼働はしない。旧serverは再確認flagを理解しない。通常の停止時保存・別DBでの更新確認と、同sourceのmigration/server/GUIを組み合わせる。Git revertだけをDB復旧と呼ばない

## 新APIとserver判定

既存path /v1/documents/{documentId}/versions/{versionId}/read-state にGET getCurrentDocumentVersionReadState、同path/viewへPOST recordDocumentVersionView、同path/resetへPOST resetDocumentVersionReadStateを追加する。旧PUTはそのまま残す。

新POST bodyはoperationId(UUIDv7、RFC variant)、expectedReadStateRevision(0〜9007199254740991の整数)の2項目だけ。additionalProperties:false。本人/観測方法/kind/任意reasonをbodyから受けない。kindはendpointで固定する。

GETはdocumentId/versionId/firstReadAt/needsRecheck/readStateRevision/isReadを返す。HumanInteractive本人、trusted contextの現在有効性、現在Read、版の所属、現行PUBLISHED、未終了をserverが一貫snapshotで確認し、private,no-storeとする。その整合したfresh200がGUIの資格hintであり、GUIの独自認可推測やmutationの最終認可/OCCの代わりではない。

POST結果はoperationId/documentId/versionId/kind/expectedReadStateRevision/changed/occurredAt/resultingReadStateを返す。resultingReadStateは初回日時/再確認flag/revision/isReadの固定snapshot。replayed等の再送で変わるfieldは加えない。これは過去の操作結果であり現在値ではない。

現在のRead拒否/所属不一致は既存404。Agent/Serviceは403。旧版・終了後の新操作は現在Readと必要なReadHistoryを先に確認してSTALE_VERSION409。旧receipt再生にも現在Read＋必要なReadHistoryが必要。期待revision不一致はREVISION_CONFLICT409、同じoperationIdの異要求はOPERATION_CONFLICT409。既に未読への新RESETと上限からの増分はBUSINESS_RULE_REJECTED422。commit結果不明はCOMMIT_OUTCOME_UNKNOWN503/retryable:true/exactRetry:trueで同path/bodyだけを再送する。

| 状態 | 要求 | 結果 |
|---|---|---|
| 行なしr0 | VIEW期待0 | 初回日時を作り既読r1、changed:true |
| 再確認未読r2 | VIEW期待2 | 初回日時保持、既読r3 |
| 既読r3 | VIEW期待3 | 同値r3、changed:false、receiptのみ |
| 既読r3 | RESET期待3 | 未読r4、changed:true |
| 未読 | 新RESET | 422、何も保存しない |
| 現在r4 | 新要求期待3 | 409、何も保存しない |
| VIEW Aの後RESET B | A完全再送 | Aの固定receiptだけ。現在未読を変更しない |
| 新版公開後 | 古い版へ新要求 | 現在認可後409、新版へ付替えない |

MAXで変更が要る新VIEW/RESETはGUIもPOST0とする。既読VIEWのno-op、保存receipt再生はserverで増分不要。GUIは既読tokenならVIEW自体を送らない。

## 原子性と監査

専用document_read_state_operationsのPKはidentity_provider/principal_id/operation_id。本人、対象Doc/Version、kind、32byte SHA256 digest、期待/結果revision、初回日時、needsRecheck、changed、occurredAtを保存する。成功receiptのfirstReadAtは必須。changedなら結果=期待+1、no-opは同値。RESETはchanged/再確認true、VIEWは再確認false。

digestはprefix document-current-read-state-v1の末尾NULと既存canonical JSON(schemaVersion1、本人provider/id、HumanInteractive、operationId、Doc/Version、kind、期待revision)を使う。receipt保持期限や汎用操作照会は追加しない。

GETはaccess共有guard→Document共有lock→認可/所属/currentの一貫read。POSTはaccess共有guard→Document FOR UPDATE→現在認可/所属/必要なReadHistory→本人receipt→現在state/CASの順。一致receiptは期待revisionを再適用せず再生。旧PUTも同じDocument lockを使う。state/receipt/必須Auditは同transactionでcommitする。

異Documentに同一本人operationIdが競合する場合も扱う。receipt INSERTのON CONFLICT DO NOTHING RETURNINGで競合側を検出し、state/Auditを含むtransaction全体をrollback。その後の新transactionで再認可してreceiptを照合し異要求409。commit失敗を確定rollbackへ読み替えない。

新Auditは実遷移時だけdocument.version.detail_viewedとdocument.version.marked_unread。payloadはdocument_version_id/operation_id/expected_read_state_revision/resulting_read_state_revision、triggerはdetail_displayまたはuser_reset。VIEWだけfirst_recordを含む。既読no-opとreplayはAuditを増やさず、旧read_confirmedを変更しない。Document revision、Domain/Search event、Audit配送基盤は変更しない。

## GUIの表示・固定要求

- QueryClient ownerのstoreで、確定した通常published/overview/workflowなしへの初回deep linkまたは外/別Docからの入場だけにopenIdを発行。history変更で離脱を同期失効。tab/hash/query/focus/refetch/resetQuery/StrictMode/remount/prefetchは新機会にしない。reloadは新ownerで新表示となる
- 入場のDoc/currentVersionを固定し、専用opening keyとstaleTime0/gcTime0/retry:false/実AbortSignalのGETで一度だけtokenを得る。15秒cacheやprefetchをtokenにしない。GETsnapshotが順序境界であり端末時計の全順序を保証しない
- 正常な基本情報/metadata/固定版filesのDOM commit後、visibleなdocument、接続/表示中の概要、現在のDoc/Version/query資格・epoch/openIdをframeと送信直前に確認。CSS非表示の祖先も拒否する。未読ならVIEWを一度同期消費して送信、既読なら消費だけ。新Versionへtokenを付け替えない
- RESETは正規の現在stateが既読である時だけ。クリックで機会を先に封鎖し、送信直前にlive成功/idle/非失効query、同じowner資格、対象/世代/現在routeを再検査する。前renderのcanResetやobject同一性だけに頼らない。same-tick失効/離脱後の古いhandlerはPOST0
- 同じtargetのpending/UNKNOWNがあれば新操作を作らない。固定intentはDoc/Version/kind/body/title/versionNo。UNKNOWNは元path/body/opIdだけで再試行。成功receiptを現在badgeへ使わない。GET一致だけでも成功と判断しない
- 初回の認識済み401/403/404/409/422は確定拒否。通信/timeout/5xx/不正receiptはUNKNOWN、UNKNOWN後の拒否でもUNKNOWNを保持。成功後read失敗は成功＋表示未確認としmutation再送へ戻さない
- Home/Detailに未確定操作回復を残し、通常read拒否や新版後も固定対象を保持。保持は同アプリメモリーだけ、未確定時beforeunload警告。localStorage永続化は追加しない
- current badgeは正規GETのみ。現在state再取得は正常時も明示buttonを提供し、openingを再発行せず未読戻しを維持。既存read resetへ同期epoch/abortを接続するが、query cache全体へ同期renderを割り込ませる監視は使わず既存useQuery購読を再利用する。他操作/Blob/Org状態は保持

新GET403は既読機能だけを閉じ、通常Document/Version/filesの成功・履歴・downloadを維持する。新GET401/404/409は通常Documentの既存queryFnで一度再確認し、その正規結果だけが既存拒否barrierを更新する。その他の補助read失敗で通常readを拒否にしない。通常read自体の失効/拒否では表示機会も停止。GET失敗をmutation UNKNOWNと混同しない。

主要文言: 本人の既読状態、既読状態、既読/未読、初回記録日時、未記録、未読に戻す、現在の既読状態を再取得、既読状態の操作結果、同じ操作を再試行。補足は「既読は文書の詳細を表示した記録です。原本の読了や同意を示すものではありません」。RESET成功は「未読に戻しました。次に文書の詳細を開くと既読になります」。UNKNOWNは「結果を確認できません。同じ操作を再試行できます」。成功後read失敗は「処理結果は確認済みですが、現在の既読状態を取得できません」。

## 検証と段階保存

新sourceでTDD/独立review/hostedを取り直す。既知のquery reset同期描画、同tick Reset、CSS非表示祖先の3反例を先行する。backendでは旧PUT混在、CAS、異Doc同IDrollback、Audit/receipt失敗、MAX、UUID variant、移行の初回日時保持を検証する。

既存runtimeのregulation/metadata/persistenceとprivate stateだけを拡張する。Human状態を共通Human/Agent snapshotへ混ぜない。metadataの表示遷移と保存/絞り込み自体の不変性を分離し、RESETの未読snapshot全体を意図した再入場の前で比較する。後続keyboard-returnが最後にregulationを開いた後に最終RESETを保存。HTTP再起動後はGUIを開く前に本人stateを照合する。

ローカルは純粋/DOM/型/build、DB系はcompileのみ。実DB/socket/Docker/Chromiumは既存hostedのみ。新runner/proxy/画像/golden/timeout/skip変更はなし。全応答喪失・DB process再起動・production Identity/TLS・対象PC・backup/restoreを未取得なら未資格と明記する。公開は同一Draft PRへ検証済み単位で保存し、未完成/失敗を隠さない。main mergeは親、実server反映は依頼者が手動で行う。

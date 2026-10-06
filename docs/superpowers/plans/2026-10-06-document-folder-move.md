# 選択フォルダーの移動を既存APIへ接続する

## 目的・基点・範囲

既存内部処理を通常GUIから使いたいという所有者の指示（`Sentinel_cef3cbfde698819198d9ed687212a610`）に沿い、PR84統合main `b9f447faa294f1898ef2b1d375b055c4b9e96cd8` からFolder移動を追加する。Frozen管理§8.3の認可/継承影響/transactionとFrozen GUI§13/§25のhint・現在read・UNKNOWNを維持する。2026-10-06 06:45/07:06 UTCに、同名衝突の既存mapper接続を含む同機能TDD・1PR完結で進める方針を確認した。

## 小さいUIと既存契約

- 対象Pは通常ツリーで実選択し、既存readSelectedFolderで元親Gの既取得範囲をfresh readする。P自身の現在moveFolder hintでフォームを提示する。直URL IDやG/兄弟のcapabilityから許可を推測しない
- 移動先Dは読める候補ツリーから選ぶ。Rootも移動先候補であり、Root自身のmove disabledを移動先不適格としない。非Rootは選択行のparent/page provenanceからfresh readする。P選択とD選択の状態を分ける
- 対象のID/取得した名前、現在親の確認済みID、移動先のID/取得した名前、理由を示す。未確認の祖先名を現在名として補完しない。「継承アクセス設定の変化により配下や自分の閲覧/編集権限が変わり得る」ことを明示し、内容を確認して送信する。完全ACL差分・影響者一覧・件数を捏造しない
- 既存POSTのbodyはoperationId/fromParentId/toParentId/expectedFolderRevision/reasonだけ。両親の期待revision・actor・ACL・preview tokenは足さない。最終の対象/両親/影響配下の認可、cycle、予約、OCCはserverが再検査する
- fresh read中のClose・選択変更・別navigationと遅延応答を分ける。送信直前も別の未解決要求を再確認し、古いreadから自動送信しない。source/destinationが変わった場合は入力を保持し、明示見直しへ止める

## 固定要求・再送・結果

- QueryClient単位の小storeでpath/body/表示contextを固定する。create/rename/moveのpending/unknownは相互保持し、新規開始だけを止める。汎用mutation基盤は増やさない
- 初回既知拒否は正規Problemのcode/status一致だけ。FOLDER_CYCLE 409・RESERVED_DOCUMENT 409も対応する。INTERNAL/503/通信断/不正200はUNKNOWN。UNKNOWN再送後の403/404/409を初回未実行の証明とせず、元の要求を保持する
- 同親no-opも現在認可/OCCをserverで確認する。receiptはoperation/resource/厳密changed/resultingRevision/occurredAtを照合し、変更時+1・no-op据置のsafe integerを確認する。replayは元receiptであり現在の親/名前/権限へ書き戻さない
- 成功receiptと現在read再取得の失敗を分ける。移動はglobal access_revisionを変えるため、改名の2ノードinvalidateだけを流用しない。Folder/Document/Organizationの認可依存read、旧cursor、query外の選択provenanceを再整合する。未解決operation store・QueryClient自体は消さず、現在navigationを元へ戻さない。WORKING・予約取消・公開状態の固定要求はQuery cache内にも存在するため、その3種の操作キーと既存WeakMap storeを保全し、現在readだけをcancel/resetする限定方式を検証する

## 同名衝突の限定TDD

moveの親UPDATEだけがmap_statement_errorを使い、一意名衝突をINTERNAL 500へ縮約する。create/renameで使う既存map_folder_write_errorへ接続すれば、既存409 / REVISION_CONFLICT契約へ戻る。新error codeや名前衝突のGUI独自認定を作らない。

1. 既存management_move_transaction.rsのfixtureで、別親にある同名P/Qの移動をConflictと期待する反例を先に追加する。親/revision/access_revision不変、該当operation台帳0、FolderMoved/監査増分0を検査する。既存management_http.rsにも409/REVISION_CONFLICTと台帳0を同fixtureで確認する
2. ローカルDB/socket/Cargoを追加実行せず、同機能Draftの既存hosted CIで実際のInternal側REDを確認する。仮説やcompile失敗を意図したREDと扱わない。失敗logを保持し、確認後にmove UPDATEのmapperだけを既存helperへ接続する
3. transaction/rollback/commit/台帳/audit/認可/replay順序は変更しない。新ID no-op・元要求replay・古い期待revisionの拒否も既存fixtureに加算し、イベント/監査/台帳重複が無いことを確認する。新fixture/runner・無制限rerun・timeout/skip緩和なし

## 実装・レビュー・資格

- regression testの意図したRED待ちと独立なGUI配線は、既存API契約を正本にTDDできる。Root移動先、同親no-op、fresh read/遅延/別navigation、固定UNKNOWN、既知拒否、旧receipt、相互未解決、現在readの再整合をpure/実route DOM/SDKで確認する。単なる見えている子一覧から名前衝突や権限を推測しない
- 既存OrganizationのFolder作成/改名2casesへの最小移動・HTTP再起動確認を検討し、201件・Root・改名・Workの既存検査を保持する。画像・trace/video・新sidecar・新runtime基盤を増やさない
- 同機能のGUI・限定mapper補修・日本語手順・試験を1本のPRにまとめる。意図したREDからGREENへ直し、独立spec/品質レビューと同head全CI・実受入・owned cleanup・公開artifact0を確認してから親がmainへ統合する。結果だけ/統合だけの別PRを作らない
- Search/Audit/Toolbox作業、ACL editor/preview、新backend権限モデル、新依存、実サーバー反映は含めない。画像・対象PC導入/backup/restore/PostgreSQLプロセス再起動/本番Identity等の未資格を保持する

# 公開前WORKING内容比較：実行状況

## 2026-10-07 01:48 UTC — 実装・ローカル検証とレビュー

- GUI所有7filesのlocal commit `83e39dd2feddd8c6806fa4fa4634551ba4c82d21`、既存受入2files `0335975f207e9cedbbbcbe2da835044723c91f78`、手動導入4docs `d40e71645ada50beb466ef1f9ed6180ed7d8d251` を同じbranchに保持。新機能の公開PRはまだ未作成。
- 専用readは結果を開いたパネルのlocal stateへ限定し、各page前後にauthoring Document/WORKINGとpublishedの固定ID詳細を照合する。既存cacheの文書/版/一覧/ACL read更新で同期世代を失効し、既存mutation storeやreset配列を変えずに旧結果を隠す。拒否だけは通常GCを跨いで保持する。
- 入口3RED、追加45REDから実装。途中のタブ/選択変更2失敗はReact key衝突を特定して修正。明示ID不在、再読取時のWORKING消失、通常cache GC後の拒否迂回も各RED→GREENで確認した。新試験fixture4型誤りと途中失敗を記録し、既存chooseVersion全体やAPIの意味を変えていない。
- 固定製品sourceの全GUI1421件/56 suites、focused215件/4 suites、schema/型/buildが成功。既存種別のwebpack性能警告3件を保持。stage検査で見つかった共有fragment末尾空行1行だけを除去し、全差分検査も成功した。独立組合せreviewはGOで、実browser/hostedはまだ未資格。
- 既存direct human journeyの公開前Version2↔3へ比較POST/結果/状態不変/close後の従来公開を追加。WORKINGを読めるhumanのsnapshotを前後で比較し、read-only agentと公開前に同一とは仮定しない。persistenceは公開済みでWORKINGがないことと入口不存在だけを追加。既存statement/case/通知/stage順序を保持し、新fixture/runner/context/proxy許可は増やしていない。
- runtime型、既存純粋81/81、MCP compile、収集18+5が成功。最初の型検査ではcurrentVersionIdがnullableの2箇所を検出し、正規公開IDが必要な明示guardへ限定修正した。VersionSummaryのtitle等を推測で補わず、実DTOのVersion詳細を使用する。
- 手動導入pinを資格済みmain41bへ同期。PR93履歴一覧入口を収録し、新WORKING比較は未収録と明記。17objects同一、Bash15例、相対link/anchor62、既存操作/旧資格保持を確認し、コマンド差は固定SHAのみ。実導入・DB upgradeを実行したとはしない。
- main41bの残証拠も補完済み。新Rust checkout一致、DB36/Folder4各一意PASS、公式Document/Organization/summary成功stepと固定main sourceで既存gate合格、公開artifact空。全13jobs/run成功は親の公式確認として区別する。runtime初回logはTransport closedで追加取得せず、raw件数/receiptを転記していない。
- 独立reviewは202件/5 suites・GUI/runtime型・docs/既存操作を照合し、実受入locatorが既存WORKING接頭辞を欠いていた1件を検出した。実routeで旧完全名の1REDを確認後、locatorを「選択中: WORKING · 版 3」へ限定修正し、同unit caseにも見出し確認を追加。修正後60件・runtime型/収集18件が成功し、controllerも最終全GUI1421/56を再確認した。製品sourceと既存試験期待を緩めていない。
- 次のexact action: 同機能Draft保存→同headの通常必須CI。実50件超・複数原本比較・再起動後の正のWORKING比較・画像/macOS・本番Identity/TLS等の未資格を保持する。main mergeは親、実反映は所有者手動。

## 2026-10-07 01:28 UTC

- 基点main `41b584ddea6c3c9ec90343f3ba98cfdac560bd24` / tree `591eb64a2d54912c2faf925b16ed70bb97265deb`。既存linked worktreeを再利用し、旧PR93枝を保持して新branch `feat/document-working-comparison-20261007` に切替済み。重複PRはない。
- [限定計画](../plans/2026-10-07-document-working-comparison.md)に従う。既存API/能力/現在readによる固定Version pairの内容比較を「版・改訂」に追加する。正式Revision snapshotを合成しない。
- 前候補Folder ACLは、継承元変更時にlocal policyRevisionが不変でもeffective grantsが変わり得る。新主体/Root不整合を保留し、今回のread-only比較を先に進める。
- 現時点は実装開始前。次のexact actionは実route反例→GUI/API配線と既存normal journey受入→独立review→同機能Draft/通常CI。実受入・画像等を完了扱いにしない。
- PR93は統合済み。main自身のCI37553290752は親の公式GETで13jobs全成功を確認した。既存source/step評価とraw stdout未取得の区別を引き継ぎ、過去の失敗や未資格を消さない。

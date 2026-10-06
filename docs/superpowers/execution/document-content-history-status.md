# 閲覧専用コンテンツ版履歴：実行状況

## 2026-10-06 19:42 UTC

- 基点main `c2b68850aa022ac77ff180e5010c2197f949036d`、branch `feat/document-content-history-20261006`。[小計画](../plans/2026-10-06-document-content-history.md)に従う。新しいworktreeをこのmainから作成し、既存PR91のsourceを取り込んでいない。
- main自身の既存push CI `37505782574` は全13jobs/checks、実Document/Organization受入・DB36/Folder4・HTTP再起動・artifact0まで確認された。今回GUIの資格へ転用しない。
- 通常「版・改訂」に独立した閲覧専用領域を追加し、既存history-purpose list/detail/files/downloadを再利用する。旧版選択を公開・編集対象から隔離し、現在認可、原本限定、選択変更・中断後の遅延応答/Blob保存抑止を検証する。
- Task 1 `31ad2ddd` は専用hook/component、既存詳細への入口と現在拒否の接続、DOM/API試験の5filesを追加した。初回37件REDから実装し、明示再読取時の余分な旧detail GET、閉じる際のfocus競合、背景再読取tailの一時失敗を明示追加失敗と取り違える境界も個別REDから限定補修した。
- Task 1最終sourceはfocused54件（DOM46/API8）、全GUI1266件/51 suites、schema・型・build・diff検査が成功。既存webpack警告3件は維持した。API試験は既存wrapperのhistory/100/cursor・原本4ID/AbortSignalの互換確認であり、backend/OAS/SDKを新実装したものではない。
- Task 2 `00a272a2` は既存regulationのjourneyとpersistenceの2filesだけを149行追加・2行変更した。通常時2版とHTTP再起動時3版から旧Version 1を選択し、history全GET、原本hash、URL/通常選択、本人ReadState、Human/Agentのauthoritative snapshot不変を検査する。新case/fixture/runner/診断fieldはない。
- Task 2は既存純粋66件、runtime型、MCP compile、収集18+5が成功。最初のjourney収集は既存dist未生成で開始前に停止し、source変更なしの通常compile後に回復した。収集・source対応を実browser/HTTP再起動合格へ読み替えない。該当旧版の原本は1件であり、実GUI複数原本も今回のfixture資格に含めない。
- 固定lockのoffline installは供給元metadata不足で停止した。通常の公式registry取得は供給元724件検査に合格し、固定687依存を再利用、download0・lock不変で完了した。検査の無効化や依存更新は行っていない。
- 全11pathsの独立仕様/品質・組合せreviewはGO。新DOM/APIとworkspace・正式改訂・比較の直接隣接5 suitesを独立実行し、244件/schema検査が成功、重大/未解消所見は無かった。手順には独立選択・明示再読取・固定導入版e249未収録とfixture限界を記録し、相対link19件を確認した。
- 次のexact action: source7filesを保持して候補凍結→日本語Draft→同head既存hosted。実hostedは未実施である。backend/OAS/SDK・新基盤・大量fixtureは追加しない。
- 明示既読はmark-read capability不足、初回複数原本登録は単数Create契約の補修が必要なため別の保留項目。今回完了扱いにしない。
- PR91はCI/check/step等の取得済み結果と、正式runtime stdout未取得を分けて記録してDraft・未mergeを保持している。今回実装の基点や資格として使わない。
- 実GUIコンテンツ版101件目、画像/macOS golden、本番Identity/TLS、対象PC導入、backup/restore、PostgreSQLプロセス再起動等の既存未資格は保持する。導入pin e249には今回機能は含まれない。
- GUI・試験・日本語文書を同機能PR内で完結する。main mergeは親、実サーバー反映は所有者が手動で行う。

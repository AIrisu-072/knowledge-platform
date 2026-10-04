# Organization Browser PoC — 差戻・再提出状況

2026-10-04 07:54 UTC。状態: **最小実装・ローカル確認PASS・限定独立レビューGO／新hosted実証待ち**。

- 所有者07:21:34 UTC「続けてください」（`Sentinel_2641c20729c08191b502766cde1a5c38`）に基づくBrowser先行の継続
- 基点: 受入PR54 remote `44e1b41219a77809f82fe22045cb4fceaf0c1ed8`、tree `daafe0ad864d883f3a72db99f491b43085bfdae9`。その実DB/2名browser/復元/cleanupと全通常CIは成功済み。受入branchは変更しない
- 作業branch: `feat/organization-return-slice`、新Draft候補。計画は[差戻slice](../plans/2026-10-04-organization-return-slice.md)。公開・実runtimeはまだ行っていない
- 固定2名・同じ一時DB/Chromium・画像非公開の既存journeyへ差戻1往復を追加。認可方式・外部送信・破壊操作・Tauri資格を拡張しない

## 実装とローカル確認

completed attempt不変履歴、ReturnInstruction、return/read API、営業再claim/事務新attempt、0002 migrationと旧record/ledger互換を実装した。既存2画面へ理由/確認/新attempt編集と、同taskIdの古いattempt応答が新attemptを上書きしないfenceを追加した。

- Work domain16、HTTP7、application1、migration1、既存Organization7: 計32 PASS。拡張した実DB試験1件はcompile成功・明示ignoredで未実行
- GUI18 suites/100 PASS。application/runtime型検査、schema freshness、production build、OpenAPI lint、architecture、対象fmt/Clippy、差分/Gitleaks検査PASS。既存Webpack advisory3件を保持
- Playwrightは既存journey1件/persistence1件の収集のみ確認。既存runtime設定のlistener-free3試験もPASS。新しいrunnerやworkflowは追加していない
- レビューで、未割当の次attemptへ移った後に既存operationのPOST再送だけが拒否される順序を修正。基本責任確認→既存ledgerのcurrent結果開示認可、新規操作はcurrent-attempt認可の順にした。旧private DraftSavedの回復は拒否を維持
- UIは旧attempt本文混入・遅い回復結果の巻戻りをRED→GREENで修正。callerからのnextTask capabilityと、受領者自身のqueue capabilityを区別して実journeyの期待値を確認

新しい合成definition versionをfresh seedへ用い、旧version・0001 checksum・旧DBの状態を上書きしない。既存のtransaction/ledger/recovery、Document機能、実runtime runnerを再利用する。

独立whole-branchレビューはGO、Critical/Importantの残りなし。上記replayのImportant1件とjourneyのactor別期待値を修正して限定再確認した。

次のexact action: exact sourceを親へ渡し、別Draftで通常CIと既存hosted実DB/2名browserの差戻往復を確認する。既知ローカルDB/listener/browser拒否は再試行しない。実動作合格は新しいhosted exact headの結果が得られてから記録する。

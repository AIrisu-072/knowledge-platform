# Organization Finding503診断の進捗

- 承認範囲：2026-10-08の親からの依頼。失敗限定の閉じた診断を追加し、小さいDraft PRで安全性試験・独立レビュー・限定runtime観測を行う。原因修正やwait/retry追加は対象外。
- 基点：main `dba816874fe5703254b9b6d67c85e1547a8b6da2`。
- branch：`diag/organization-finding-503-20261008`。
- 分離：専用worktree。他checkoutと旧作業内容を保全。
- 重複確認：open PRの該当backend／Organization runnerに重複なし。PR112のsupport.tsは変更対象から除外。Directory／loadの別担当ファイルも除外。
- 実装：Finding専用task-local診断と、503 Finding失敗に限定した安全なharness抽出。Document provider内部のtimeout／DB障害は今回細分化していない。
- ローカル検証：固定Node24.21.0・canonical TMPDIRでOrganization Node全40件成功。閉じたRust診断moduleを実sourceで単体実行し2件成功。Organization TypeScript、node構文、rustfmt、diffcheck成功。新しいRust async4件とfull Cargoはhosted CI確認待ち。Macの4GiB容量reserveを維持し、重い全workspace buildは実行していない。
- TDD：閉じたRust JSON反例とharnessの不正入力／503 gating／失敗保持でREDからGREENを確認。既存source契約testはinline読取から変数への変更と503 gatingだけを調整し、元のassertionを保持。
- 現在：独立レビュー・Draft PR・正確headのhosted runtime観測待ち。503解決済みとはしていない。
- 次のexact action：独立レビューを確認し、同一Draft PRへ保存、正確なheadで既存runtimeを実行する。
- 未解決：再起動後Finding503の実際の失敗分岐。本番ホスト、認証、TLS、実機配備は未実施。
- 操作手順：[診断の利用と解釈](../../operations/organization-finding-failure-diagnostics.md)。

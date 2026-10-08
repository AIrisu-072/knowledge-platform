# Organization Finding503診断の進捗

- 承認範囲：2026-10-08の親からの依頼。失敗限定の閉じた診断を追加し、小さいDraft PRで安全性試験・独立レビュー・限定runtime観測を行う。原因修正やwait/retry追加は対象外。
- 基点：main `dba816874fe5703254b9b6d67c85e1547a8b6da2`。
- branch：`diag/organization-finding-503-20261008`。
- 分離：専用worktree。他checkoutと旧作業内容を保全。
- 重複確認：open PRの該当backend／Organization runnerに重複なし。PR112のsupport.tsは変更対象から除外。Directory／loadの別担当ファイルも除外。
- 実装：Finding専用task-local診断と、503 Finding失敗に限定した安全なharness抽出。Document provider内部のtimeout／DB障害は今回細分化していない。
- ローカル検証：固定Node24.21.0・canonical TMPDIRでOrganization Node全40件成功。閉じたRust診断moduleを実sourceで単体実行し2件成功。Organization TypeScript、node構文、rustfmt、diffcheck成功。既存キャッシュでwork-repository-postgresの全lib11件（新しいasync4件を含む）も成功。Macの4GiB容量reserveを維持し、重い全workspace buildはhosted CIで確認した。
- TDD：閉じたRust JSON反例とharnessの不正入力／503 gating／失敗保持でREDからGREENを確認。既存source契約testはinline読取から変数への変更と503 gatingだけを調整し、元のassertionを保持。
- 保存先：[Draft PR114](https://github.com/AIrisu-072/knowledge-platform/pull/114)。診断コードhead `53be1715c25a83fce4d9e04135ce36cf7f0535b1` の独立レビューは重大指摘なし、GO。独立Node全40件も成功。
- 正確な診断コードheadの[CI37761551047](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37761551047)は全14項目成功。追加の条件付き2項目はSKIP。Document全22段階・Organization全23段階成功、dirty=falseのhead一致を確認。Finding503は今回再現せず、失敗診断も出力されていない。原因が解消した証拠とは扱わない。
- 現在：上記観測結果を残す文書だけの更新。診断コードは同じで、原因修正・wait/retry追加・main統合は未実施。文書更新後の最新head CIはPR Checksで確認する。
- 次のexact action：最新headの独立差分確認と必須CIを確認し、親へ「原因未特定」を報告する。修正に進む前に、同一診断コードで追加の限定観測が必要。失敗分岐を観測できないまま原因修正を提案しない。
- 未解決：再起動後Finding503の実際の失敗分岐。本番ホスト、認証、TLS、実機配備は未実施。
- 操作手順：[診断の利用と解釈](../../operations/organization-finding-failure-diagnostics.md)。

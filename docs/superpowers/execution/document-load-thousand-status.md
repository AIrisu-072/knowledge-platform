# Document 1,000件検証の状況

Status: COMPLETE / 実1,000件成功、1万・10万件は別段階

2026-10-09 02:34 UTC追補: PR117をmain `5f9f4756f565b46a62b39f5eaea1d857221b0943` へ統合し、統合後CI `37870372080` 全14成功を確認。専用run `37872406514` はfresh small成功、1,000件の容量判定通過、1,000件登録・公開、非対応原本422拒否、更新競合、公開切替、認可拒否、再起動後保持、最終終了処理まで成功した。所要942,290.206ms、標本RSS666,656,768 bytes、保守的disk増加139,239,424 bytes。receipt SHA256 `2d09b513264b738915fd4af5ebd821ee45b2dac7f5165affd2fe487a37ed559b`。大規模時の原本/履歴詳細照合は3件の標本であり、本番SLOや全原本ハッシュの検証とは呼ばない。続きは[1万・10万件の状況](document-load-ten-thousand-status.md)。以下は実装時点の記録。

- 基点main: 4fa12dc4777feaa30315f0230a148aac9ebfcc8b（PR110の統合後CI14成功）
- branch: test/document-load-thousand-20261009
- 承認範囲: 元の段階負荷検証、明示opt-in180分job/120分stage作業中止期限。通常45分CIと既存安全係数は維持する。
- 設計・手順: [計画](../plans/2026-10-09-document-load-thousand.md)、[README](../../../tools/document-load-qualification/README.md)

2026-10-09 00:58 UTC: fresh small→1000 chain、stage別のFolder/journal、2回の再起動世代、PID/DB/storage/source連続性、success-only限定receipt、manual-only workflowを実装。新機能は実RED→GREENで検査した。独立reviewは観測不能な未admission理由の問題を指摘し、固定codeと数値だけの診断を追加して解消。scope内の残指摘なし。

Cloud Node24.19.0（pin24.21.0とは別）でハーネス184件・既存runtime177件が成功。添付された既存small receiptも変更後のvalidatorで再検証して一致。Rust/製品API/sandbox/admission predicateは変更していない。次は正確headのDraft PR通常CIと独立review結果を揃えて統合し、public/標準runner/同一main headを再確認したmanual1000 runを1回だけ起動する。fresh smallが120分stageや実測容量へ収まらなければ1000を開始せずNOT_ADMITTEDを報告する。

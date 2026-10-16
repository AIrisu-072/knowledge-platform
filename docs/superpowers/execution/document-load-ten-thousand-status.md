# Document 1万・10万件検証の状況

## 2026-10-15 — 現在状態（main `ead6830e` を照合）

[実1万件run 37887357388](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37887357388) はhead `d996f8012ef7ebe57e5fc47b49bd85d26d02441a`、qualify jobとrunがSUCCESS。small→1,000→10,000の3段階が成功した。大量段階の原本・履歴・再起動snapshotの詳細照合は各段階3件の標本で、繰り返した公式PDFによる合成負荷である。全原本の品質・全件hash照合・製品SLO・本番資格を示さない。

PR118はmain `d996f801` に統合済み。Ubuntuの10万件資格は未取得であり、この別runのreceiptをUbuntuのfresh admissionへ流用しない。次の段階は[local状況](document-load-local-scale-status.md)を参照。

以下は各作業時点の履歴。未統合・未実行等の記述は、その節の日付時点の状態である。

Status: ACTIVE / 1万件の実装中、実1万・10万件は未実行

- 基点main: `5f9f4756f565b46a62b39f5eaea1d857221b0943`（PR117統合、統合後CI `37870372080` 全14成功）
- branch: `test/document-load-ten-thousand-20261009`
- 設計: [1万件計画](../plans/2026-10-09-document-load-ten-thousand.md)
- 2026-10-09承認: 「1万・10万件も実施してほしい。これが本番と同じ規模なので」。専用360分jobと絶対期限、終了処理予算15分、既存安全係数を維持する設計を独立レビューした。

1,000件run `37872406514` / job `113633253442` は成功。fresh small成功後に容量判定を通過し、1,000件登録・公開、非対応原本の422拒否、更新/競合、公開切替、認可拒否、再起動保持を確認。所要942,290.206ms、標本RSS666,656,768 bytes、保守的disk増加139,239,424 bytes。成功receipt43,421 bytes、SHA256 `2d09b513264b738915fd4af5ebd821ee45b2dac7f5165affd2fe487a37ed559b`。全件原本ダウンロード照合や本番SLOの認定ではない。

2026-10-09 02:51 UTC: 1万件向けの段階連続性、provider開始時刻による残時間判定、限定的な終了処理上限、3段階receipt、走査の観測値をTDDで実装した。Cloud Node24.19.0でハーネス317件・既存runtime177件が成功し、YAML構文・差分検査も成功。独立レビューはGO。通常CI/既存1,000件workflowと製品コードは差分なし。GitHub公式のbare pathと厳密な@main形式をテストし、実際の過去run attemptがbare pathだったことも照合した。次は正確headのDraft PR CIを経てmainへ統合し、実行を一度だけ開始する。実1万件は未実行。

10万件は実1万件の測定と実行場所の資格が必要。現在のMac空き約6.25GiBでは不足する。利用者がRAM32GB・空きstorage2TBの候補機を提示したが、OS/CPU/接続状態およびLinux sandboxは未確認。新規credential、ネットワーク設定、有料資源は作成していない。

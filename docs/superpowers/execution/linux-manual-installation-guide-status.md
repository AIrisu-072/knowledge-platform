# Linux手動導入手順書の状態

## 2026年10月4日 UTC

- 対象：所有者がLinuxサーバーへ手動導入するための[日本語手順書](../../operations/linux-manual-installation.md)
- 基点：PR57 `d383baccddd5081687b500f064f6fce195a24816`。独立branch `docs/linux-manual-installation-20261004`
- 範囲：架空データ限定の固定2名Browser PoC、秘匿設定、新しい専用DB、明示migration/bootstrap/seed、起動、停止、更新、backup/restore、切戻しと本番未達チェックリスト
- 対象PCはCore Ultra 9 285K、Linux方針のみ確定。distribution/version、実接続先、容量、運用設定は未確定
- Agent次slice、Search、Audit、Tauriの完成や本番稼働を主張しない。進行中branchは変更しない
- 静的検証：既存CLI・config・runbookとの照合、Bash例12個の構文、相対リンクの存在、追加2文書だけの変更範囲、staged diff検査、既存repository policyがPASS。コマンド本体、新たなDB・listener・browser・credential・deployの実行なし
- 実機での手順一式、backup/restore、PostgreSQL再起動後の受入は未実施。基点のhosted CIはこの新しい手順書の実行証拠ではない
- 限定文書レビュー：統合担当が全文と現CLI/config/health/初期化を照合しGO。全writer停止、DB/storageの一組保存、元を残す別DBへの復元、本番未達の表示を確認した。補足の静的修正はPDFium取得失敗の伝播とPostgreSQLのTCP readiness指定のみで、構文と元harnessの方式を再確認済み
- 現在：文書のみの新Draft公開とexact-head CI確認の準備完了。基点や進行中の他branchを変更せず、公開後の実結果はPR本文に記録する。対象PCでの実手順は未検証のまま、OS決定時にOS固有の準備を追記する

## 統合調査の注意

2026年10月4日14時台UTCのGitHub照合ではmainは `d71753d46590bb4406a1c0b74894ab90a27a6c88`、main CI成功。PR36〜57のうちPR52/53は標準CI失敗。PR40の `0610b49327cd3c1c37e385281f087423c27b5638` は標準CI・DSI・Sandbox全成功だが、P7とSearch全体受入は別gateである。

DocumentとSearchはそれぞれDocument migrationのversion 9を追加している。適用済み台帳を確認せず番号・checksumを変更しない。GitとDB/storageのrollbackを分離する。既存のGitHub Actionsに自動production配備は見つからず、GitHub environment/deployment履歴はconnectorの読取対象外で確認できていない。実本番接続・配備は実施していない。

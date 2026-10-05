# WORKING複数原本編集の小計画

[承認済み条件の追補](../specs/2026-10-05-document-working-version-editor-amendment.md)を実装する。元完成sourceは `feat/document-working-version-editor-20261005` の `f639fbf0`（base公開main `5d262557`）として保持する。レビュー可能な境界に分け、D1 `feat/document-working-manifest-api-20261005` はbackend/API/SDK、D2はD1へstackするGUI/runtimeとする。各公開前に最新合格mainの履歴を保持して統合する。別sliceの未公開metadata/予約取消は流用しない。公開mainへ後で両履歴を保持して統合する。

1. backendの初回更新・stale/予約/公開終了条件・nullable結果/ledgerと正確manifest readをTDDで追加する。API原本とtyped clientも同時に更新する
2. 既存単原本新版フォームを全manifest共通編集へ置き換える。全取得・保持・選択原本とその変換物だけ変更する。取消、二重送信、失効、遅延、unknown同payload回復を純粋GUIでRED→GREENにする
3. 既存hostedの固定2合成profile、PostgreSQL18.6、Chromium、owned cleanupへ複数原本編集と再起動確認を追加する。新specのtop-level recording設定はscreenshot/trace/video全off。既存有限診断だけを拡張し公開artifactを0に保つ
4. 最終全GUI・型・build・API contract・純粋Rust/oneshot・DB試験compile-only・runtime collection-onlyを実施し独立レビューする。日本語packetと正確commit/treeを親へ渡す。公開/CI/mergeは親、実サーバー導入は所有者の手動操作

重要な検証点: 2原本と各変換物の保持、1原本だけ差替え/対象変換物だけ除外、正確な元名、途中取得失敗、parts/size/共有ID、初回nullと以前公開nullの区別、OCC/権限失効、unknown固定bytes再送、編集中現公開維持、publish rollback/atomic切替、再起動復元。

ローカルDB/socket/listener/browserは実行しない。Rustは専用target・jobs2で他Cargoと排他。依存は既存lock/storeの通常installを使用し、root node_modules symlinkやcheck無効化を使わない。

## 2PRへの分割境界

D1は初回更新条件・capability整合・正確manifest read・nullable結果とledger・schema/SDK・backend試験を含む。binary transport変更、GUI部品、runtime新ケースはD2へ残す。D1単独でも従来GUI・API型・既存runtimeが成立することを確認するが、安全な複数原本GUIが有効になったとは主張しない。

D2は全manifestフォーム、固定操作/bytes回復、既存新版単原本導線の置換、表示と共存、実runtime2+1を含む。元f639を改変せず、分割後の全製品sourceが元featureと一致することをpath/blobで照合する。段階別記録以外の仕様意味や機能を変更しない。

# 選択フォルダー移動：実行状況

## 2026-10-06 07:13 UTC

- 基点main `b9f447faa294f1898ef2b1d375b055c4b9e96cd8`、branch `feat/document-folder-move-20261006`。[小計画](../plans/2026-10-06-document-folder-move.md)で既存POST/read/hintのGUI配線と、同名衝突の既存mapper接続だけを範囲とする
- PR84の作成日時GUIは公開head `28181b32` / tree `b9769bf7` で全18checks（15成功/既存skip3）、CI `37424855884` 全13jobs、GUI870/39、Document18+5・実カレンダーGET/開始包含/終了除外/詳細往復/再起動/本人Agent readState不変、Organization・Agent9・DB36・cleanup・全4run artifact0成功。main b9自身のpush CI `37427490836` も全13checks/jobs・今回mainの同実受入・指定DB36・終端artifact0を確認した
- Folder移動はread-only調査と小計画まで。新permission projection/ACL差分previewは不要で、現在hintを特定移動先の成功保証としない。同名衝突Internal化はsourceで確認した仮説で、実DBのREDはまだ未取得
- 07:27 UTC追補：test-only `b00b372d` で既存DB/HTTP fixtureへ衝突2反例と、成功replay・新ID no-op・no-op replay・stale no-opの確認を追加した。rollback/台帳/監査を先に照合し、DBは固定ラベルinternal/conflict、HTTPは500/409の差を検査する構成。指定2filesだけのrustfmt・diff checkは成功。compile/実DB/実REDは未取得で、mapper本体は未変更
- 次の操作：この反例先行sourceを独立reviewし、同機能Draftの既存hosted CIへ載せる。実際のInternal/500側REDを確認後に既存mapperへ限定接続する。GUI・受入・日本語文書はこの同じPRで完成させる
- Folder移動の実装/試験/実受入はまだ未取得。旧PR82 persistence失敗とPR83 Organization503の未解明記録、画像・全headers喪失・対象PC/本番Identity等の未資格を保持する

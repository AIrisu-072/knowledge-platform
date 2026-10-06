# 選択フォルダー移動：実行状況

## 2026-10-06 08:37 UTC — 実装と受入sourceの組合せ

- 基点mainは `b9f447faa294f1898ef2b1d375b055c4b9e96cd8`、同機能の[Draft PR87](https://github.com/AIrisu-072/knowledge-platform/pull/87)で試験・GUI・手順をまとめる。新しい結果専用PRは作らない
- 最初の公開head `db9031dba1f9ae851d2ba8147ab705067f7f30ae` / tree `c034166f2a173e80c14be7d3237995350da76b80` は反例先行。[CI37431473978のRust試験](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37431473978/job/112163100089)で、HTTP同名衝突は操作台帳0通過後に実500/期待409の差で失敗した。compile/fixture失敗ではない。fail-fastでRepository衝突と追加no-op/replayは未実行なので、2反例のREDや全rollback合格とは扱わない。全18checksは13成功/2失敗/既存skip3、全4run artifact0。旧GUIの実受入成功を移動GUIへ転用しない
- 実REDの後、move親UPDATEの1箇所だけを既存 `map_folder_write_error` へ接続した。新error code・認可・transaction・commit・replayの順序を変えず、既存409/REVISION_CONFLICTへ写す。stale no-op拒否後にも元のFolder/Document/access snapshot一致を検査する
- 通常ツリーの実選択、別の移動先ツリー、理由と継承影響確認、固定要求のUNKNOWN再送、現在read再取得を実装した。Rootは移動先として明示選択でき、同親no-opもserverへ送る。新navigation/遅延read/同batchの旧移動先送信/入力保持を実DOMの反例から検査した。移動後に対象行が隠れた場合のfocusも、既存改名と同じ可視Root fallbackで補修した
- 最終ローカルGUIは952件/42 suites、schema/型/build・diff check成功（既存webpack性能warning3件）。GUIと限定mapperの独立spec/品質レビューはGO、独立251件/8 suites・schema/型・Rust format成功。focus補修を含む最終組合せレビューは次の受入sourceと合わせて確認する
- global access_revision後はDocument/Folder READと、Organizationの文書・根拠・Agent結果・提出snapshot READを更新する。identity/task選択とprovider、3種のquery-backed固定操作、既存WeakMap操作、登録回復markerは保持する。古いDocument URL cursorを無言で捨てず、既存の明示再適用を維持する
- 実受入sourceは既存Organization 2+2の中に移動1回・現在親の実GET・fresh 200+1 cursor・通常ナビ往復・HTTP2process再起動後の元receipt再送を追加した。純粋28件・runtime型・GUI schema/型・MCP compile・収集2+2は成功。組合せレビューで見つかった旧親のGUI読取失敗を空表示と取り違える検査の穴は、HTTP503を見逃す実反例から必須fresh GUI GET200/空/null cursorとalert0の照合で補修した。純粋28件・型・収集2+2も再確認し、限定再レビュー中。実browser/DBのGREENはまだ取得していない
- 導入4docsの現pin `0801c986` には未読・日時・今回移動が未収録。公開製品headの実受入と全CIが合格した後、そのheadを同PR内の4docsへ固定して最終CIを確認する。自己参照は避ける。Document/Organization CLI・env・migrationは旧pinと同bytesで、公開main側で更新済みのCargo lockをそのまま使って既存手順の再buildを行う
- 次の操作：組合せ独立reviewの指摘を閉じ、同PRの公開headで全CI・両衝突反例・既存no-op/replay・移動実受入・owned cleanup・artifact0を確認する。mergeは親担当、実サーバー反映は所有者の手動作業
- macOS golden/画像、全status・headers喪失時のWORKING、対象PC、本番Identity/TLS、backup/restore、PostgreSQL process再起動は未資格。今回の同Root継承・文書なしの移動fixtureを、実ACL変化やGUI通信断の資格にしない。旧PR82 persistence失敗とPR83 Organization503の原因未特定の履歴は保持する

---

## 2026-10-06 07:13 UTC

- 基点main `b9f447faa294f1898ef2b1d375b055c4b9e96cd8`、branch `feat/document-folder-move-20261006`。[小計画](../plans/2026-10-06-document-folder-move.md)で既存POST/read/hintのGUI配線と、同名衝突の既存mapper接続だけを範囲とする
- PR84の作成日時GUIは公開head `28181b32` / tree `b9769bf7` で全18checks（15成功/既存skip3）、CI `37424855884` 全13jobs、GUI870/39、Document18+5・実カレンダーGET/開始包含/終了除外/詳細往復/再起動/本人Agent readState不変、Organization・Agent9・DB36・cleanup・全4run artifact0成功。main b9自身のpush CI `37427490836` も全13checks/jobs・今回mainの同実受入・指定DB36・終端artifact0を確認した
- Folder移動はread-only調査と小計画まで。新permission projection/ACL差分previewは不要で、現在hintを特定移動先の成功保証としない。同名衝突Internal化はsourceで確認した仮説で、実DBのREDはまだ未取得
- 07:27 UTC追補：test-only `b00b372d` で既存DB/HTTP fixtureへ衝突2反例と、成功replay・新ID no-op・no-op replay・stale no-opの確認を追加した。rollback/台帳/監査を先に照合し、DBは固定ラベルinternal/conflict、HTTPは500/409の差を検査する構成。指定2filesだけのrustfmt・diff checkは成功。compile/実DB/実REDは未取得で、mapper本体は未変更
- 次の操作：この反例先行sourceを独立reviewし、同機能Draftの既存hosted CIへ載せる。実際のInternal/500側REDを確認後に既存mapperへ限定接続する。GUI・受入・日本語文書はこの同じPRで完成させる
- Folder移動の実装/試験/実受入はまだ未取得。旧PR82 persistence失敗とPR83 Organization503の未解明記録、画像・全headers喪失・対象PC/本番Identity等の未資格を保持する

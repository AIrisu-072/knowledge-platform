# 専用 ext4 DB の PG18 起動修正

Status: ACTIVE / ローカル修正・実機資格確認中、未公開

- 基点 main: 818e8b079b7ddda76462026010424afa589159ad。独立 branch fix/document-load-pg18-ext4-20261009。
- 承認範囲: 専用の新規テスト DB だけの runner 修正、回帰試験、Ubuntu ext4 起動・通常 runtime・small。長時間4段階は最終修正 head のレビュー・CI・main統合後に親と調整する。
- 原因: PG18 の既定 PGDATA は private bind root の子で、公式 entrypoint が root を chown しない。UID1000/mode700 root を UID999 postgres が通過できず Permission denied。実機の固定診断・読取り専用 probe で確認。
- 修正: owned bind のみ PGDATA=root を明示し、公式 entrypoint に新規 directory の chown を任せる。mode700、外側親の所有権、ext4、inode、単一 mount を維持。通常/small の ext4 canary に明示 local opt-in を追加、hosted と外部 DB 併用を拒否。既定 hosted tmpfs 不変。
- TDD: PGDATA 引数と explicit opt-in の RED→GREEN を確認。独立レビューは重大0、中程度1（Docker CLI失敗後cidfile回収）を検出し、finally回収を追加して再確認中。
- 検証: Mac全load試験はLinuxの/proc・launcher・RSS依存8件で失敗。成功へ読み替えず、固定依存のUbuntuで全suiteを確認する。実Docker回帰は明示opt-in時だけ動く。通常runtime/smallの実結果はこれから。
- 次: clean修正headをbundleで専用Ubuntuへ渡し、旧PG18設定の正確な失敗log、新設定のDB起動・回帰suite・通常runtime・smallの結果を保存する。公開が拒否された場合は停止し別経路を使わない。既存サービス・データ・認証・.npmrcは触れない。

2026-10-09: 未公開c51e64a7/tree1b8f4596のUbuntu clean checkoutでload456成功/実機opt-in1skip、runtime191成功、DB canary成功（旧設定の正確なmkdir Permission denied、修正後18.6/mode700/UID999、CLI失敗後cid回収・削除確認）。通常runtimeは全段階・owned DB cleanupまでpassed。smallは実行中。独立exact-headレビューは重大・中程度0。観測説明が試験規模で分岐しているため、ext4 opt-in smallをtmpfsと記述する誤りを追加RED→GREENで修正した。実測値を否定するものではないが、c51の結果を新headの成功へ転用せず、最終headで回帰・通常・smallを取り直す。長期4段階は未開始。

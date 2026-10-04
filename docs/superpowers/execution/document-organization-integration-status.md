# DocumentとOrganizationの統合状況

## 2026年10月4日 UTC 現在の範囲

状態：**文書側枝の統合済み、限定ローカル検証と独立保持レビューはGO、新exact-head CIは未完了**。実装は[PR57](https://github.com/AIrisu-072/knowledge-platform/pull/57)の受入済み `d383baccddd5081687b500f064f6fce195a24816` から変更していない。統合のための新機能・依存・migration・workflow・画像公開は追加しない。

所有者は2026年10月4日にmergeを許可し、実サーバーへの反映は自分で手動実施するため手順書を希望した。本候補ではmain merge前にhead/tree/CI/副作用を親担当へ渡し、同時操作を防ぐ。実本番接続・deployは実施しない。[Linux手順書](../../operations/linux-manual-installation.md)は固定2名の合成PoCを対象とし、本番IdentityとOS固有準備は未達のままである。

## 保全する履歴

- main基準：`d71753d46590bb4406a1c0b74894ab90a27a6c88`。統合branch：`integrate/document-organization-20261004`
- 初期基点：PR58 `6118a690d0eb7f8e16d6fe2c12ebbb641a81b016`。mainとPR43→47→48→49→54→56→57はすべてその祖先
- 文書側枝のmerge parent：PR36 `706307b980beb540db0759ad772ed4debd42c289`、PR39 `7d49a7bbde4d26bd072732c41cebe85a419fd4dd`（PR38 `5a2b114964ddbe7d38dd6a5fe9b70fdad2cb56f1`を包含）、PR46 `88628331f32e141c71944e9f7f093deb348f51ac`、PR55 `3a99c9ffc19bf7493b4c83e913b92dbba6eff152`
- PR43の受入対象は `6103e4d4e3bb0d45ba03e1d2935492de7f11394a`、最終報告はPR46の上記head。[依頼者の最終受入記録](https://github.com/AIrisu-072/knowledge-platform/pull/43#issuecomment-5952167525)を保持する
- PR54 `44e1b41219a77809f82fe22045cb4fceaf0c1ed8`、PR56 `cf28175d9b2467afd7225fa4f92f1d7a801d4002`、PR57の実DB/2名操作/再起動後復元は、それぞれのexact-head証拠のまま保持する
- 全既存branch/refは移動・削除しない。進行中AgentはPR57基点を維持する別作業であり、この候補には入れない

## 競合の判断

- `.gitleaksignore`：PR36の31件に対し、PR57は別途承認済みの正確な4fingerprintを追加したsuperset。PR57のblob `62ba16075a480c7a99c42ce21dab297a12625197` をそのまま保持し、例外追加・削除・拡大をしない
- Active：PR57の全履歴を出発点に、PR36/46/55の固有checkpointを追加。過去の失敗や保留を削除して現在の成功へ置き換えない。統合中の機械的な競合処理で末尾欠落を検出したため、元の950行から再構成し、側枝の追加分・末尾・再開節を照合した
- PR38/39のstatus：元のA1/E0本文はaccepted側に既に全内容が含まれることを確認し、後続の記録を残したまま祖先を保全した
- PR46：受入報告、画像制限、runbookの限定upload説明を統合。報告sourceと製品sourceを混同しない
- PR55：最小Browser PoCの実証完了を追加し、後続の差戻・根拠・人間判断の説明とローカル実行制限を保持した
- 日本語化：既にレビュー済みの説明文書3件だけを元の日本語blobで採用。[対応表](../../translations/2026-10-04-organization-integration-ja.md)。凍結design/approval原本は不変

## G9と残るgate

PR36自身の旧headについて、same-head frontend E2Eの欠落があった事実を消さない。後続PR43の実composition/画面受入が明示承認され、PR57までの受入済み経路をこの候補へ取り込んでいる。新しい統合候補の同一headで実browser/backendを含むCIが成功して初めて、その統合経路の検証済み状態を記録する。過去のPR36へ結果を遡及しない。

既存の画像レビューは正確な元sourceだけの証拠であり、新画像の撮影はしない。既存画面sourceと新機能の分類、制限付き受入はそのまま維持する。Search全体、Audit store/delivery、Tauri/native、production Identity、Agent実モデルは本統合の合格対象ではない。

## 検証と次の操作

- 全13対象PR headを祖先として包含：PASS
- PR57に対する非文書ファイルのpath/blob/mode集合一致：PASS。製品・lock・migration・workflow・scannerに追加差分なし
- 凍結design/approval原本、既存訳文3blob、Active履歴、変更文書の相対リンク、repository policy、diff：PASS
- 固定Node24.21.0/pnpm12.4.1：Organization純粋helper14件、GUI19 suites/129件、API contract12件、app/Document runtime/Organization runtimeの型、schema freshness、production GUI build、Organization API lintがPASS。既存Webpack advisory3件は維持。offline frozen installはpackage metadata不足で停止したが、同じ固定lockの通常installはsupply-chain policy724件に合格し、687packageをcache再利用、package download0件で完了した
- ローカルRust全体、DB/listener/browserは実行していない。上記の限定検査で実runtimeを合格とはしない
- 独立保持レビューはlocal `ee3b4dcda5971c6d49babf236dc5cae6bef802f7` / tree `3c00da49bd5334980d2eed125a5c82aec7f3ed36` に対しGO、Critical/Important指摘なし。非文書1,224 entries、既存spec/plan61件、13対象head、旧Active950行、報告・手順書・翻訳blob、相対link140件、workflow副作用を確認した。その後の変更はこの検証結果と計画checkboxの更新のみ
- 公開は最終treeと、順序付きの既存parent5件（6118a690、706307b9、7d49a7bb、88628331、3a99c9ff）を照合する。公開head/treeと全適用CIは未完了。最終結果は新Draftの本文へ記録し、古いheadのCIを付け替えない
- 次のexact action：main baseの新Draft公開→全適用exact CIの終端確認。main実merge前に親へhead/tree/差分/CI/副作用を返す

mainへmergeすると既存のpush CIが動作する。新しいdeploy workflow、image push、production migration/seedは追加しない。通常の`serve`もmigration/seedを実行しない。Gitの切戻しはDB/storageや外部副作用を戻さない。

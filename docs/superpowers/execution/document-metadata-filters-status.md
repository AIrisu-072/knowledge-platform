# 文書一覧の属性絞り込み：実行状況

## 2026-10-06 01:56 UTC

- 基点はPR80統合main `1fe1b011e31477cd7a4de1b7facdcef56a97612d`、branch `feat/document-metadata-filters-20261006`。[小計画](../plans/2026-10-06-document-metadata-filters.md)を固定し、既存3属性queryのGUI配線を開始する
- PR80改名はhead `57e5d121` / tree `9a80a475` で全18 checks（15成功・既存skip3）・通常CI `37399045746` 全13jobs・限定実改名/HTTP再起動・cleanup・全4run公開artifact0を確認してmerge済み。新main push CI `37401300371` も全13jobs/checks・実改名/HTTP再起動・DB36・cleanup・公開artifact0成功
- 属性値はtrim/NFCしない完全一致。URLが正本、条件変更時cursor破棄、詳細往復の復号と有限長を最小補修する。新backend・権限推測・Search作業はない
- 02:16 UTC追補：製品 `e57042fe`、受入 `2f9eb7b2` で固定。TDD focused83/4、同sourceHEADの全GUI768/39 suites・GUI型/schema/build・両runtime型・既存MCP build・Organization純粋28・metadata用途guard3・collection2+2/18+5が成功。既存webpack警告3件を保持する
- 不正属性URLの非string/孤立surrogateは日本語route errorでGET停止。通常formの不正文字列は入力保持と理由表示。詳細の不正returnToも条件を捨てて一覧へ戻さず停止する。標準router以外のserializerや新基盤を追加していない
- 次の操作：日本語docsとの組合せを独立レビューし、同じtreeでDraft公開と既存hosted全CI・属性の実GET/HTTP再起動・cleanup・公開artifact0を確認する。実runtime資格は未取得。短い合成値の実一致/不一致と長さ/Unicode/遅延応答の純粋DOM資格を区別する

## 2026-10-06 02:30 UTC — 独立レビュー指摘の限定補正

- 初回組合せ `dbb44f00` は、他の不正URL条件による既存fallbackが新属性を捨てるImportant 1件で公開前に停止した。`59912a97` で型/Unicode確認済みの新3属性だけをfallback時にも保持し、空文字省略と旧条件の既存挙動を維持した
- 修正前のpureと詳細復帰DOMで9件の意味的REDを確認。直URLの代表例はrouterの既存mergeで元からGET停止できていたため、直接通信の不具合とは混同しない。旧試験を保持し14件追加、同sourceHEADの全GUI782/39 suites・focused97/4・型/schema/buildが成功した
- runtime/collection/pure対象は前の資格対象 `2f9eb7b2` と同bytes。実hosted資格は引き続き未取得。計画の正本節番号を§11へ訂正し、次はこの限定修正と日本語文書の再レビューから同一head Draft/hostedへ進む

## 2026-10-06 02:37 UTC — 空欄URLの送信境界補正

- I1と節番号の所見は閉鎖。限定再レビューで、直URLの空属性がrouterのmergeにより復活しAPIへ空値を送るImportant I2を確認した。公開前のまま、`ec05d6f8` でHomeのquery keyとGET入口6行だけを空文字未指定へ揃えた。space等の非空値、SDK、既存validator、I1補修は変更していない
- 実route DOM6件で空キー送信とcache分離のREDを確認し、修正後は同sourceHEADの全GUI788/39 suites・focused103/4・型/schema/buildに成功。実HTTPの422試験を実行した資格とは区別する。runtime等は前の資格対象と同bytes
- 次はI2差分だけの独立再確認から、日本語Draft・同一headの既存hosted実受入へ進む。まだ今回の公開/実runtime資格は未取得

## 2026-10-06 03:28 UTC — PR81とmainの統合資格

- [PR81](https://github.com/AIrisu-072/knowledge-platform/pull/81)のhead `426db3a5a43083a1b4bd76b656ea58322019eed3` / tree `794407fc62e2418bd6ecbdc2de4541cd3e207b02` はI1/M1/I2閉鎖・独立レビューGOで公開。全18checks（15成功/既存skip3）、通常CI `37405694808` 全13jobs、全4run公開artifact0成功を確認してmergeした
- main `0801c9864bdb7faf5fcbe7ee1062367335ee7bfb` は同tree、parents `[1fe1b011,426db3a5]`。[main自身のpush CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37407454204)も全13checks/jobs成功。GUI788/39、Document18+5、属性の実GET/詳細往復/HTTP再起動、Agent9、Organization2+2/build含む8段階、Rust1831と既存別21/7、今回DB36/36、owned cleanup、公開artifact0を新しい実ログで確認した
- metadata persistenceの個別公開summaryは上限で一部省略されるため、固定spec/configと全5成功・skip0も照合した。長さ/Unicode/複合URL/遅延応答のDOM資格と実の短い合成値によるGET資格を分け、既存visual・対象PC手順等の未資格を維持する
- このsliceの統合後確認は完了。手順pin4docsの更新と、別branchの公開一覧未読条件を次作業とする。実サーバー反映は所有者の手動操作

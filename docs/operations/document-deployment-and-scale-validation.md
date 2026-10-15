# 文書管理の配備先確認・復旧演習・大量文書追試

2026-10-08の準備記録。基準sourceはmain `2a37d35cd228344f98e0194de16d5336fa786e3c`。接続中のMacは開発環境であり、配備先ではない。この追補は配備完了・本番資格・容量合格を主張しない。

## 配備前に依頼者が決めること

秘密値をチャットやrepositoryに提出する必要はない。次の非秘密情報を確認してから、対象host専用の実施手順と承認対象を確定する。

| 確認事項 | 必要な回答 |
|---|---|
| 対象機 | OS/version、CPU architecture、RAM、利用可能disk、既存service、管理者・運用責任者 |
| 接続方法 | 作業を誰が実施するか、許可された管理接続の方式・時間帯。鍵/passwordそのものは不要 |
| 利用範囲 | 同一PCのみ、LAN、VPN、外部公開のどれか。利用者数、同時利用、DNS名の有無 |
| 認証 | 組織Identity基盤と管理者、利用者/group同期元、失効・退出・障害時の運用 |
| TLS | DNSと証明書の管理責任者、発行/更新方式、reverse proxy管理者、信頼するproxyの範囲 |
| 復旧 | backup媒体と容量、暗号化・鍵管理責任者、保持期間、許容データ損失RPO、復旧時間RTO |
| 受入 | 作業停止時間、合成/公開文書を使用できる検証領域、停止条件と切戻し判断者 |

接続用accountの作成・永続権限追加、実account/groupへの権限拡張、firewall、TLS、認証、service常駐、実DB migrationは、具体的な変更内容を示して実行前の確認を得る。既存のPoC固定利用者を本番認証として使わない。アプリ内の利用者検索機能の実装は、この実account設定とは別である。

## 再現可能な検証と受入順序

1. 受入対象commit、exact-head CI、architecture、locked toolchain、DB image digest、PDFiumのOS/architecture対応を固定する。[Linux手動導入手順](linux-manual-installation.md)はその掲載release専用で、SHAだけ置換しない。新releaseとのmigration・設定・worker・GUI差分を比較する。
2. 対象機の実データと分離したDB・storage・portを確保する。原本は公開資料または合成資料とし、既存processを停止/変更せず、検証の所有資源だけを扱う。
3. [実Document runtime検証](../../tools/document-poc-runtime/README.md)を通常buildで実施する。`--prebuilt`は診断専用で完全合格にしない。登録・再送・公開履歴・原本hash・ACL・HTTP再起動後保持の結果をsource/runとともに保存する。HTTP再起動、接続proxy障害、PostgreSQL process再起動は別試験である。
4. 停止backupと別DB/storage復元は既存手順の節8–9を使う。Document、Organization、scheduler、Search配送等の同一DB/storageへ書く全processを止める。DB dump、storage全体（`staging/`・`objects/`・`work-artifacts/`）、設定、release、worker/PDFiumを一組にし、保管先へ秘密を出力しない。
5. 復元前後で文書/版/改訂/公開履歴IDと状態、全原本のhash/size/order、ACL、利用者×公開版の既読/初回日時、予約状態、Work保存内容を照合する。元DB/storageは保存し、復元先へbootstrap/seedを繰り返さない。dump/storage検証不一致なら起動と更新を止める。
6. 所有する検証DB processの正常停止→再起動後、同じ照合を再実施する。別途異常停止試験を行う場合は停止方式と許容影響を先に確認する。復元所要時間、backupサイズ、停止時間を計測し、決定済みRPO/RTOと照合する。
7. 本番認証/TLSは別の受入単位とする。未認証拒否、偽装主体headerの無効化、group/利用者失効後の一覧・取得・書込拒否、Identity障害時の安全な拒否、証明書/DNS/更新・期限・proxy経由の境界を対象hostで試験する。方式未決定のため、この追補で認証adapterやTLS設定を選定しない。
8. 独立レビューと受入を終えたreleaseだけを承認された対象hostへ適用する。更新後書込を含むDB/storageをGit revertで戻せるとは扱わない。切戻しは既存手順節10で確認する。

## 検索作業との重複を避ける

GitHub PR #86/#99の現在状態・headは実行前に担当者と照合する。2026-10-08のこの環境の`gh pr view`はnetwork errorでlive状態を取得できなかった。以下はローカルに取得済みremote refの読取であり、PRの最新head/合格の証明ではない。

- `origin/feat/search-validation-corpus-20261006` = `fa5fe36e6861df26cd418933070af98ba365ae4e`。`experiments/search-validation-corpus/scripts/ingest.py`はCommon Document APIによる初回登録・改訂読取・公開とmanifest保持を提供する。並行登録器を作らず、担当者とmanifest形式・所有DB/storage・sourceを固定して再利用する。
- 同branchの状態記録は999 unique文書/999原本、版更新未試験、金融庁告示PDF0件を報告している。この歴史的記録を今回の実測に転用しない。e-Gov法令API入力はXMLからtext/plainに変換され、公式PDFの検証とは区別する。
- `origin/feat/search-generation-segments-20261007` = `ce1798b91e6df48312b7340770dad10b8da258b1`。Search世代分割や索引性能の修正・measurementは検索担当へ残す。新migrationを無断で統合しない。
- 共有検証環境への書込/再起動/復元、索引再構築、大量投入は担当者の実行枠と調整する。Document-onlyとSearch配送ありを別runにし、配送backlogやCPU/容量の影響を記録する。

## 文書管理の段階測定

最初は公開の公式法令・告示PDFを10–30件程度で確認し、版固定URL、取得日時、SHA-256、bytes、出典・再利用条件、PDFページ数をmanifestへ保存する。PDFiumのhost対応を先に確認する。取得不能・署名/暗号化/破損/未対応は原因ごとに記録し、成功したtext/plainへ無言で置換しない。Wikipediaは件数・本文長負荷の補助資料として来歴を区別する。大量に複製した同一PDFはunique文書/unique原本数を区別する。

1000→1万→10万件は候補段階であり、既定の達成目標ではない。各段階前に容量見積り（DB/index/storage/staging/backupの合計＋作業余裕）、CPU/RAM、停止条件、投入上限、実行枠を確認する。ディスク逼迫や未確定結果、hash不一致、履歴破壊、権限漏れなら次段階へ進まない。

| 操作 | 確認する意味・測定 |
|---|---|
| 初回登録 | 文書数/原本数、本文長/bytes分布、成功/拒否/結果不明、重複なし、複数原本の順序 |
| 一覧・取得 | 先頭/中間/最終page、重複/欠落、ACL別可視数、取得hash、cold/warm各応答時間 |
| 更新・公開切替 | sampleの複数原本追加/削除/並替、OCC競合、再送時1操作、旧版原本と改訂履歴保持 |
| 公開/取下げ/予約 | 現行と旧公開版の選択、履歴不変、現行切替、予約実行/競合/権限喪失 |
| 権限 | 可視/不可視の別利用者、継承/明示ACL、失効後一覧/原本/改訂拒否、拒否時内容漏れなし |
| 保持・復旧 | HTTP/DB process再起動・別DB/storage復元を区別し、manifestと全sampleを照合 |
| 容量・性能 | operationごとのp50/p95/max、試行数/同時数、wall time、CPU/RSS、DB/storage/index/backup bytes、配送backlog |

sampleは先頭/中間/末尾、最小/最大bytes、多原本、旧公開版、不可視文書を含め、選択seedとIDの対応を私有evidenceへ保存する。warmupを集計から分け、成功応答だけのlatencyで失敗を隠さない。未完了・結果不明は別件数とし、独立レビュー前に合格を付けない。p95は試行数と算出法を添える。SLO/許容上限未決定なら数値だけ報告し、合否は未判定にする。

各runの記録は日時、source SHA/tree、依存/toolchain/image、host仕様、corpus manifest hash、投入済/未済数、sample seed、操作/試行/同時数、raw evidenceへの非秘密参照、測定値、失敗/未実施、後片付けの所有確認を含める。実ID・原文・資格情報・私有pathをrepositoryの集計へ含めない。

## 今回の検証境界

このMacの空き容量は観測時約8 GiB。Dockerのread-only情報取得はsocket権限拒否となったため、この環境ではDB実受入・復元・1000件以上の投入を実施していない。対象PC導入、本番認証/TLS、公式PDF実投入、容量性能の新規実測はいずれも未実施。既存CIや検索担当の過去測定で埋めない。

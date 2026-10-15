# Ubuntuでの文書10万件検証

## 目的と実行境界

利用者が求めた本番と同じ件数の検証を、専用Linux x86_64環境で実施する。1万件・10万件とも実際の同じDB/原本storageへ登録する。小さな独立試験を合算したり、別runnerのreceiptを現在環境の容量証明に読み替えたりしない。GPUは不要。

この変更は検証harnessの明示的なlocal modeだけに適用する。製品API、PDF検査、sandbox、認可、通常CI、既存のsmall/1,000/1万件workflowの既定動作を維持する。GitHubのイベントや環境変数を偽装しない。ホスト全体の設定・サービス・credentialを変更しない。

## 同一環境の4段階

専用のclean checkoutと固定依存からbuildし、普通の実HTTP/GUI/MCP・PDF sandbox資格を通過した後、small2件 → 1,000件 → 10,000件 → 100,000件を実行する。各段階は別folder/journalを持ち、前段の文書を次段の対象件数に含めない。正例累計111,002文書、各段階の負例を含め111,006文書、次版原本4件を含め111,010原本ファイルに既存fixtureが加わる。

直前のfull reportと、さらに前の全履歴を検査する。code/corpus/runtime/run UUID、DB/storage identity、現在のPIDと前段の再起動後PIDが一致することを確認する。4回の所有HTTP/workerプロセス再起動を行う。未知の初回登録結果を自動再送しない。

## 時間・容量の予算

起動時に全体80時間を固定し、最後15分を負荷中止後の処理・終了・結果保存に予約する。small上限5分、1,000件120分、1万件330分、10万件72時間と残時間の短い方を各段階の上限にする。単調時計の経過時間を併用し、時計が後退しても使える時間を増やさない。

係数2、RSS上限2GiB、disk予備1GiB、利用可能memory予備512MiBは変更しない。10万件を開始するにはfresh1万件の所要時間×20が72時間以下かつ全体残時間以内、標本RSS×2が2GiB以内で、現在のdisk/memory観測も満たす必要がある。開始不可は未完として報告する。80/72時間は運用停止予算であり製品SLOではない。15分もOSやfilesystem停止時の終了を保証する値ではない。

## 専用ext4 DB

local modeでは最初のsmallから、所有run directory内に新しいDB directoryを作り、既存PostgreSQL18.6 containerの`/var/lib/postgresql`へ限定してbindする。実体path・所有directory・ext4・symlink不在を確認し、稼働後はDocker mountのtype/source/target/RWとdirectoryのdevice/inodeを照合する。公開するidentityは固定modeと一方向hashだけで、pathは出さない。途中でDB/storageを切り替えない。

既存admissionはstorageとDB filesystemの小さい空き容量へ総増加予測を照合する。2原本のサイズだけでfresh1万件は1,107,465,000 bytes、次の10万件は係数2込み22,149,300,000 bytes＋reserveを要求する。小さいtmpfsを増やすためにRAMを圧迫せず、専用ext4を用いる。既存hosted枠はtmpfsのまま。再起動資格はHTTP/workerの再起動で、DB containerやホストの再起動耐久性ではない。

## 測定器のメモリ上限

長寿命serverのstdout/stderrは、秘密値がchunkやUTF-8境界をまたぐ場合も除去してdiskへ逐次記録する。メモリには上限付き末尾のみを保持し、書込み失敗は成功として扱わない。既存の完全captureが必要な短いprobe commandは維持する。

local modeの資源観測はprivateなappend-only sidecarへ保存し、件数・bytes・digestを記録する。メモリには最大256件の最近の観測とonline集約だけを保持する。欠けた観測が窓から消えても測定値を有効へ戻さない。タイミングはoperationごとの数値列とstatus件数に整理し、従来どおり全標本から厳密なnearest-rank percentileを算出する。保存/flush失敗を成功にしない。

毎秒という観測目標、全storage走査の10秒/100万entry上限、現在容量による停止判定を維持する。観測にかかる時間・entry数は実測し、古い論理サイズを新しい観測として渡さない。全件一覧は10万件/1,000page境界まで検証する。索引や製品queryの変更は実測で必要性が判明するまで混ぜない。

## 起動・切断・停止

起動helperは専用ext4 evidence root、cleanな期待HEAD、固定Linux環境を確認する。所有lockを排他取得し、当初の開始時刻から80時間以内を目指すwatchdog付きの単一process groupをSSHから分離する。起動準備で消費した時間も差し引き、最後の120秒より前にTERM、その120秒以内に終了しなければKILLを送る。送信やOSの停止処理が実行できたこと自体は別に観測する。PIDだけでなく開始tick・boot ID・run UUID・sourceを記録し、runtimeの起動ackを照合する。既存tmuxやsystemd serviceは変更しない。

再接続時は同じrunを読み取り確認し、二重起動しない。切断耐性は小さいcanaryで実証してから長時間runを始める。停止は所有資源だけに限定する。timeout、再起動、worker消失、不明な登録結果を部分失敗として保持する。自動的な途中再開や未知POSTの再送は実装しない。

## compact receipt

旧receipt形式は変更せず、local用の閉じたschemaを追加する。生成前にprivate full reportの実ID、件数、一意性、段階間非重複、順序、完全なpreviousReport参照、DB/storage/PID/time連続性、実測値を検証する。正例IDは小文字UUIDを辞書順に並べ、各行LF・末尾LFのUTF-8列のSHA256と正確件数を固定JSONへ保存する。詳細検証の標本件数と`allOriginalHashesVerified:false`を明示する。

成功receiptは実際の最終server停止・proxy終了・所有container削除が確認できてからだけ生成する。privateな全ID/report/journalとDB directoryは保持する。compact receiptからadmission用full reportを復元しない。digestは集合の完全性であり実行主体の真正性や全原本内容の一致を単独で証明しない。公開uploadは追加しない。

## 検証と実施順

期限/容量境界・mode混在・source不一致・mount差替え・4段階連続性・pagination限界・chunk境界の秘密値除去・大量観測のメモリ・終了失敗・receipt改変をTDDで検査する。独立レビューと正確headのCIに成功したclean commitをUbuntuへ渡す。実UbuntuのPDF sandboxと通常runtimeを確認し、最初のsmallから全段階を開始する。実測の結果を確認するまで10万件を成功と呼ばない。

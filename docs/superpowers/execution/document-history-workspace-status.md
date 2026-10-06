# 履歴一覧から旧版・原本・イベントを開く：実行状況

## 2026-10-06 22:10 UTC — 初回hosted失敗と既存smokeの限定補修

- [PR93](https://github.com/AIrisu-072/knowledge-platform/pull/93)の初回head `cd67bc7aa42dc31b64a2dd8aac7ffcd706c627f0` / tree `98d3713e17978c5961935cbdd618b6a481d25c19` は、凍結21filesと一致することをreadbackした。[CI37536784720](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37536784720)のDocument runtimeは失敗し、必須gate未合格である。
- 公式stepはcheckout/依存準備/Chromium成功、composition失敗、Organization未実行、bounded summary失敗。元の接続済みlog tool初回と時間を置いた1回の通信復旧はいずれもTransport closed。check outputも空で、失敗case・到達phase・cleanup stdoutを取得できていない。未対応annotation URLや以前の公開GET403を別経路で回避せず、失敗を成功stepから補完しない。
- 同じ製品sourceをCI=trueで確認し、全GUI1351/54、schema/型/build、preview純粋36が成功した。runtime helperのDTO/日時/原本名・bytes/既読と監査のsource再点検でも追加の確定矛盾は無かった。これらをhostedの失敗原因解明や実browser合格とは扱わない。
- composition前段の既存e2eに、今回のナビ追加で成立する別の確定回帰を発見した。`e2e/document-workspace.spec.ts` の2箇所がlink名「文書」の部分一致を単一要素として検査し、「文書履歴」にも一致する。実AppShell DOMで同じ部分一致の単一queryが複数要素となるREDを保存し、完全名と各URLを確認するGREENへ変更した。実Playwrightの失敗stdoutと同じ原因だったかは未確認のままである。
- e2eの2locatorを`exact: true`へ限定し、既存keyboard caseでは「文書履歴」の存在確認も加えた。caseの操作・成功条件を削らず、製品source、timeout/retries/skip、golden・画像設定を変更しない。AppShell1件とschema、e2e6件の収集が成功。独立reviewも限定2testfilesの変更とAppShell1件を確認してGO。次は同PR次headで通常CIを確認する。実browser資格はまだ得ていない。

## 2026-10-06 21:44 UTC — 実装とローカル検証

- GUI commit `c11471c7508e34fd9a89395d94f652ddaac5a5a8`、既存受入拡張 `130de682` と実投影修正 `1d415680`、導入手順更新 `54040d50`。同じ機能branch内で固定し、新PR/hosted CIはまだ未作成。
- 通常navigation「文書履歴」から、freshなhistory一覧の明示行だけを閲覧専用panelへ接続。代表版とDocumentのended/現在属性を区別し、既存content/event hook・AUTHORITATIVE取得を再利用した。イベント表示だけを小さい共有componentに抽出し、通常詳細は従来の契約を保持する。
- 39件の新実route反例で、normal detail/変更APIを使わないこと、拒否後自動retry200、明示再読取、遅延一覧/Blob、paused/失効、条件/page/選択/close/focus、UNKNOWN/Blob/Organization contextの保持を確認した。全体read resetの同期失効後に旧panelが再取得を開始するraceは、既存2hookへ任意の現在read guardを渡して修正した。全体resetの期待値や意味を緩めていない。
- 固定GUI sourceの全1351 tests/54 suites、focused326/6、schema/TypeScript/production buildが成功。独立reviewは基点からの全diff検査で抽出componentの末尾空行1件を検出し、この空行だけ除去して全差分を再確認した。webpackの既存性能警告3件と、既存イベント拒否observerのno-queryFn警告は保持する。最初のRED、途中の型不適合/fixture期待/取消race等の失敗も記録し、全consoleが無警告とはしない。
- 既存2lifecycle journeyと1persistence caseを拡張し、通常入口→同ID選択→旧原本hash・イベント・状態不変を検査する。case数18+5、fixture/runner、timeout/retries、画像off設定は不変。runtime型/MCP compile、純粋81（safe66＋画像なし配線15）、18+5の収集が成功。VersionSummaryにtitleが無い実server契約を確認し、型guardによる誤った前提を除去してHistoryDocument.titleから取得した。実browser/DBはこれからで、収集成功を実受入へ読み替えない。
- 導入pinは資格済みmain933d/tree8cへ更新。旧e249の時点別資格を保持し、新しい履歴一覧GUIがpinに含まれないことを明記した。対象17source objectsが旧pinと同一、Bash15例の構文と相対リンク57件を確認。コマンド変更は固定SHAのみで、実導入・DB upgradeの資格は追加しない。
- 基点main自身のpush CI37530751555は21:21:51 UTCに全13jobs/13checks成功で完了。fresh Rust/DB36/Folder4、公開artifact0、main/tree/両parentを確認。runtime stdoutはTransport closedで未取得、既存の強制終了条件と実checkout/Document/Organization/summary成功stepの対応により必須gateを評価した。source上の選択件数と実印字値を区別する。
- 独立SOURCE/組合せreviewはGO。230 tests/5 suites、schema、基点からの全diff、同17objects・Bash15例・追補後の相対リンク58件が成功した。製品sourceの最終差は上記末尾空行1行だけであり、機能suiteの再実行は不要と確認した。
- 次のexact action: 同機能Draft保存→同headの通常必須CI/hosted受入。main mergeは親。新機能のhosted・画像・100件超等は現時点で未資格。

## 2026-10-06 21:20 UTC

- 基点main `933d3b0f894e610496022defae8e494b16de39ea` / tree `8c6789bc3ae0332894ab1dea8f1b84686444a611`、branch `feat/document-history-workspace-20261006`。[小計画](../plans/2026-10-06-document-history-workspace.md)に従う。
- PR91統合head `63ab9728` は全13jobs・18checks中15成功/既存skip3、fresh Rust/DB36/Folder4・4run artifact0を確認してmainへ統合された。runtime stdoutは未取得で、固定sourceが強制する条件と当該checkout/Document/Organization/summaryの公式step成功から必須gateを評価した。実測件数や個別receiptを直接読んだとは記録しない。
- main自身のpush CI37530751555は別に監視中。新mainのruntime stepはsuccessで、元tool初回ログはTransport closed、追加取得せず同じ必須gate契約で対応評価した。残Rust/最終artifact等は監視中で、新機能の資格へ転用しない。
- 現serverはHistoryDocument一覧とhistory-purpose版読取を持つが、通常navigationから履歴一覧へ入り、公開終了/全版取下げ後の行を選択して閲覧するGUIが無い。通常Document detailにはhistory用途が無いため、そのrouteへ混ぜず一覧内の閲覧専用領域へ接続する。
- 現在Document metadataと版固有metadata、endedとVersion状態、nullを区別し、通常404とhistory認可拒否を同一視しない。既存read/control/原本監査と未確定要求の保持を再利用する。
- 次のexact action: 小計画固定→反例先行GUIと既存lifecycle受入の最小拡張→独立review→同機能の日本語Draft/同head既存CI。backend/OAS/SDK・fixture/runner・新基盤は追加しない。
- Folder既存主体ACLと公開前WORKING比較は後続候補、正式改訂単体詳細は今回含めない。明示既読・初回複数原本・新規主体追加・終了後管理の不足を完了扱いにしない。導入pin e249には今回機能は含まれない。
- 実GUI100件超、画像/macOS golden、本番Identity/TLS、対象PC導入、backup/restore、PostgreSQLプロセス再起動等の未資格を保持する。main mergeは親、サーバー反映は所有者手動。

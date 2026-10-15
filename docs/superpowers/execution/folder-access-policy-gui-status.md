# 非root Folderアクセス設定：実行状況

## 2026-10-07 03:10 UTC — 最終sourceと独立レビュー合格

- GUI本体33b02f210、限定補修f52af90d275105ae3fbc6fc8c32b8743c8ce9abdの所有20filesを固定。最終全GUI1523件/60 suites、schema/型/production buildが成功。既存webpack性能警告3件を保持する。runtime2files24164e4f＋日本語label5c17ed8d、導入4docs89adb009と合わせて同じ機能PRへ収める。
- 独立全体reviewで2件を補修した。継承切替の確認欄は未知の上位権限差分を表示せず、継承中の主体欄は確認した変更前の読取値、個別設定は保持した変更案と区別する。比較query単独の失効→同値の高速再取得では旧Blobを新readで救済しないよう、開始時の正確なquery stateを固定し同期購読で中断する。
- 比較単独失効の反例はRED→GREEN、全体ACL resetと通常原本の対照3件は元からGREEN。全体ACL resetを突破したとは記録しない。独立4反例の再実行、限定5file hash照合で両所見解消・仕様/品質GO・未解決所見なし。継承/個別を往復した変更案の保持も検証した。
- Unicode表現が異なる別主体IDをlocaleCompareが等順にする反例から、集合比較を正確な文字列順へ補修した。APIの主体IDをNFC同一視せず、行順差だけのno-op receiptを誤って結果不明にしない。途中候補1515/1516、architecture失敗、中断した全体runを最終1523件の資格へ混ぜない。
- 日本語操作節と現行5節の導入pin表記を整合し、過去の資格・操作・復旧本文は保持した。固定導入版d515はPR94比較を含み、今回ACLは未収録。新backend・共有Shell・Tauri・Organization・Audit・Searchの機能変更はない。
- 次のexact action: 最新mainと同branch既存PR有無を再確認し、候補を固定→同機能Draft→同headのhostedと全必須CI。今回の実browser受入はまだ未資格。実通信断/自己失権/並行親変更、画像/macOS golden、本番Identity/TLS/対象PC/backuprestore等を合格扱いにしない。

---

## 2026-10-07 02:55 UTC — 固定候補の最終検証と独立レビュー

- GUIは実Home入口/API/DOMの反例から実装。固定要求の深い保持、UNKNOWN後403/409、主体三つ組、親変更による同revision実効差異、選択変更・fresh read直後の同期失効を検証した。最終候補の全GUI/schema/型/buildは進行中で、実hostedは未取得。
- 途中全GUIの1件失敗はcomponentのSDK型直接importが既存architecture契約に反したものと確認し、既存facade参照へ補修。初期/root入口のhidden期待に対するdisabled表示も独立受入reviewで見つけ、保持済み非root要求の回復入口を残してDOM反例から非表示へ修正した。失敗を最終成功へ読み替えない。
- ACL保存後の同tickに旧原本Blobが保存される反例を確認。共通read resetの最初のawaitより前にreadだけを同期失効させ、通常原本componentと使用中の正式比較原本buttonへ既存AbortSignal/現在read guardを接続した。比較側は既存hookの固定pair判定を使い、別URLやquery keyを推測しない。未使用button、Organization、Shell、Tauri、backend/API schemaには変更を加えない。
- 既存受入2filesは24164e4f、権限名の日本語整合5c17ed8dで保存。合成Sharedの2groupで履歴権限off→復元→継承→個別設定、固定payloadと既知成功replay、root/別個別ACL/文書の不変、既存private stateの最終GET→HTTP再起動後再readを追加。runtime型・純粋81/81・collection18+5、実保存helperのfield保持、元case/statement保持を確認した。実の通信断・自己失権・親との並行変更を資格したとはしない。
- runtime2filesの独立reviewは仕様/品質GO、組合せ入口不一致は上記GUI側で修正。最後の日本語label整合を含め、全体reviewと新hostedで確認する。
- main d515自身のpush CI37562024089は13jobs/13checksすべて成功、Fresh Rust本人checkoutと1849/21/7、DB36/Folder4各一意PASS、artifact0を確認。runtime stdoutはTransport closedのまま、固定source＋公式Document/Organization/summary成功stepから既存必須gateとowned cleanupを評価し、印字値を捏造していない。
- 導入4docsは89adb009で資格済みd515へ同期。17objects/62files照合、Bash15/相対link67、旧資格と復旧手順保持を検査。新ACL操作節を同機能へ追加し、この操作はpin未収録と明記する。次のexact actionは最終source検証→独立全体review→最新main確認→同機能Draftと同headの通常CI。main mergeは親、実反映は所有者手動。

---

## 2026-10-07 02:31 UTC — 限定設計承認とTDD開始

- 基点main d515aa38085c9ed7e41f8103d9c1a6c576025fd4 / tree29f75ce9。既存linked workspaceを再利用し、旧PR94枝を保持したまま `feat/document-folder-access-policy-20261007` へ切替。新PRはまだ無い。
- [小計画](../plans/2026-10-07-folder-access-policy-gui.md)の非root・既存主体の編集/削除と継承切替が実装承認済み。新directory/Root意味変更/実データACL変更は無い。正規GETと現在認可/OCC、全grant削除禁止、深い固定payload UNKNOWN、自己失権後の不明結果保持を守る。
- PR94はhead d612d25fのGUI1421/56・独立review・全必須CI13jobs/18checks（15success/3非適用skip）・DB36/Folder4・artifact0合格を日本語本文へ記録して統合済み。runtime stdout未取得は公式step/固定source評価と区別して保持する。main d515自身のpush CI37562024089は別監視中でまだ未資格。
- 次のexact action: 実route反例→GUI/store/API配線→合成fixture実受入の最小追加→独立review→同PR docs/tests/hosted。今回操作の実browser資格は未取得。main mergeは親。

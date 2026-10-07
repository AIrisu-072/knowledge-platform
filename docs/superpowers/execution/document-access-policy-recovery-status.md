# 文書アクセス設定の回復補修：実行状況

## 2026-10-07 05:08 UTC — 最終GUIと独立レビューの確認

- GUI commit 2b6cd7785e3329b7a21b5fe8d1dffcda804cceeaの固定16filesでfocused307/5、全GUI1583/62、schema/型/buildが成功。既存warningは保持し、途中1580の資格とは区別する。
- 固定25filesを独立レビューし、仕様/品質ともGO。専用53/2、全25 SHA-256一致、diff空白、Bash15例を独立確認した。未実施hostedを過去形で記した手順1文は将来の確認範囲へ訂正し、実施済みと扱わない。
- 公開前にmainがPR95統合93947f3dde533ae0f67588637f4afeea02b3e17cへ進んだことを確認。Documentとの共通変更はActiveの追記だけで、両側の全文を保持する通常統合を行う。他担当のRuntime/Shell/CI追加はmainのまま保持し、その機能の改変・独自資格実行は行わない。
- 次は最新mainとの組合せsource確認・既存GUI/型/build→同機能Draft→新headの通常CI。今回のhosted実受入はまだ未資格。導入pinは既に資格済みbbaを保持する。

---

## 2026-10-07 05:00 UTC — 実wire反例の補修と最終検証

- 実DocumentDetail routeと実SDKのPUT JSONで、応答喪失→背景policy再取得→再送時にoperationId・expectedPolicyRevision・権限配列が変わるREDを確認した。最初のquery通知を待たない偽GREENを別に保存し、描画反映後の実wire差分で確定。新runnerやnetwork接続は作っていない。
- 既存Folderの固定要求/receipt検査を小helperで再利用し、Document専用storeと「文書アクセス設定の保存結果」の回復表示へ接続。自己失権で通常Access tabがなくなってもHome/Detailから自分の固定要求を確認できる。現在のpolicy拒否では古い主体表示を隠し、送信時の要求と現在値を区別する。
- 同値no-op、UNKNOWN後403/404/409、入力・tab・一覧往復・同tick重複、未送信の同revision実効変化、関連Folder/移動要求との相互保護、read/Blob失効を検証した。旧成功fixtureの固定op/revisionが厳密receipt検査に合わず全体2件失敗した段階を保持し、実送信に対応する合成receiptへ補修した。途中候補を最終資格へ流用しない。
- 04:52固定候補は全GUI1580/62・schema/型/buildが成功。その後、管理権限だけの喪失を通常Document GETのerrorへ変換する反例と、既存拒否barrierを迂回する読取境界を追加確認し、正規detail observerのqueryFnを再利用する限定差分に補修した。通常閲覧成功と管理可否を分け、実のGET拒否は既存履歴/比較barrierへ伝える。新receipt日時もJST翌日反例から既存formatへ揃えた。
- 最新固定GUI16filesは専用53/2、focused307/5、schema/型/buildが成功し、全GUIを最終実行中。既存webpack性能warning3件は保持する。今回の実hostedは未資格、次は独立全体reviewと同headの通常CI。
- 既存受入2filesは5304fba2197b0dacefaf5d229c881ea3d53ea9c5へ保存。Document policy1PUTの固定body/receipt・既知成功replay・通常navigation往復の結果保持/確定close・private stateの最終GET→HTTP再起動readを追加。型/MCP compile/純粋81/収集18+5、Folder/Documentのprivate policy保持と既存case/画像条件/公開添付保持を確認。実UNKNOWN喪失/自己失権の再現とは区別する。
- 導入4docs d945a90b994b93d49e0d100217feb80ebf1a25d6は資格済みbbaへ同期し、Folder ACLを収録・今回Doc回復は未収録と明記。17objects/91files、新Rust一意PASS、Bash15/リンク74、旧資格/操作/復旧本文を検査した。新回復手順を同じ機能へ追加する。
- main bba自身のpush CI37570945202は全13jobs/13checks成功、本人Rust/DB36Folder4・artifact0確認済み。runtime stdout未取得は固定source＋新公式successstepでの既存gate評価と区別する。今回補修のsourceや資格へ旧結果を付け替えない。

---

## 2026-10-07 04:36 UTC — 限定補修と反例作成

- 基点main bba1d6dd45d93c5ad52e4a69debc9a3e77e5a8ab / tree84b0b0d4。既存linked workspaceを再利用し、旧Folder ACL枝を保持して `fix/document-access-policy-recovery-20261007` へ切替。
- [限定計画](../plans/2026-10-07-document-access-policy-recovery.md)に従い、既存Document AccessTabの結果不明要求の固定・往復保持・関連read失効だけを補修する。新主体/権限意味/本文/新backendは変更しない。静的所見を実route REDで確認してから実装する。
- PR97 Folder ACLは全GUI1523/60・独立review・実受入/必須CI合格後に統合済み。main bba自身のpush CI37570945202はDocument/Organization/summary成功、runtime stdout初回Transport closed、残CI監視中で未終端。今回補修へ旧資格を転用しない。
- 既存API残件の棚卸しでは、既読は小capability補修が必要、原本追加削除並替と旧版取下げ/正式改訂detailは別候補、初回atomic複数登録/新主体directory等は新契約が必要と区別した。これらはこの補修に混ぜない。
- 次のexact action: operationId/bodyが変わる実反例→既存Folderパターン再利用→合成受入/日本語手順→独立review→同機能Draft/同head CI。新しい実装・実hostedの成功はまだ記録しない。

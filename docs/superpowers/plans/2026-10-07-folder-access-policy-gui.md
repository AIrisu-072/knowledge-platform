# 非rootフォルダーのアクセス設定：限定GUI実装計画

> 実装担当はTDDと独立レビューを使う。既存操作を再利用し、実装・試験・日本語手順を一つの機能PRへまとめる。

## 目的と承認済み範囲

既存Document APIを通常GUIから利用できるようにする継続指示に従う。PR94統合main `d515aa38085c9ed7e41f8103d9c1a6c576025fd4` / tree `29f75ce9c8bc2c9d283a2ae9484529bcb8d6555d` を基点とする。フォルダーツリーで選択した非root Folderの現在のアクセス設定を読み、既存主体の権限編集・削除、継承と個別設定の切替を行えるようにする。保存前に変更内容と影響範囲を表示する。

仕様正本は `spec/` の現行Document AccessPolicy契約。Frozen GUI designの能力表示・主体三つ組・UNKNOWN保持を守る。今回の限定案は親から実装承認を受けている。System Rootの保護・能力の既存不整合、新主体directory、手入力による主体追加、新backend、実データのACL変更は扱わない。

## 制約と意味

- 対象は通常ツリーで選択した行のFolder ID、元親ID、取得済みpage範囲を根拠に再読取できる非rootだけ。URLだけから対象を推測しない。既存childrenのmanageAccess能力とGET access-policyの正しいtargetを確認する。serverによる最終administer認可が正本。
- 編集可能な主体は、そのFolderの新しいGETで確認したeffectiveGrantsの `(subjectKind, identityProvider, subjectId)` だけ。principal/group/roleはいずれも既存型のまま扱う。表示名解決失敗は識別子へfallbackし、表示名をmutation targetへ使わない。読み取っていない主体を追加しない。
- read/readHistory/write/publish/administerの既存5権限を編集し、行の削除を明示する。1行の空actionsは削除扱いとし、全grant削除は現spec/domain/OASに従い禁止する。ReadからReadHistory等を推測しない。
- inherit→explicitは「確認した実効grantを基に個別設定を保存する」。保存前のfresh readでtarget/能力/Folder revision・parent/設定方式/local policyRevision/effective sourceとgrant集合を照合し、差があれば変更を再確認する。親ACL変更やFolder移動がlocal policyRevisionを増やさない既存契約を保持し、読取からcommitまで継承主体が不変である原子的保証は付けない。
- explicit→inheritは、その時点の上位の設定を再び適用する操作。子孫のうち継承する範囲にも権限変化が及び得ること、独自ACLの子孫へ同じ効果を断定できないこと、自分の閲覧/管理権限を失い得ることを保存前に示す。既存APIにない正確な子孫数/ACL差分previewを作らない。
- operation ID、target、expectedPolicyRevision、mode、grants、reasonを初回送信前に深く固定し、UNKNOWN時は同一要求だけを再送する。往復・別Folder選択・再表示・GCで破棄しない。成功応答喪失後にadministerを失うと再送403が返り得るため、これを元操作の失敗証明にしない。GETの一致を成功receiptへ読み替えない。
- 成功receiptとその後の読取結果を区別する。成功後に自分の読取/管理権限がなくなっても成功を撤回せず、現在readは非表示/拒否のままにする。古いpolicy/文書/履歴/比較/原本/Organization Document contextを再利用しない。保持するUNKNOWNやBlob、選択providerは消さない。
- backend/SDK/schema・依存・runner・画像設定・既存skip/timeoutを変えない。新規ローカルDB/socket/browser実行なし。合成fixtureだけの既存hosted受入を使う。main mergeは親、実サーバー反映は所有者手動。

## Task 1: 既存APIの画面操作と回復

対象: `apps/document-web/src/api/document-api.ts` の既存SDKへの薄い2wrapper、`application/document-folder-access-policy.ts` の固定要求store/検証、`components/document/FolderAccessPolicy.tsx` の表示と確認、`routes/DocumentHomePage.tsx` の入口。既存Folder create/rename/moveとDocumentMoveの未確定要求を相互に保護する必要最小限の配線を含む。既存FolderMove read resetへ新policy readを接続し、Document AccessTabのmutation処理を転用しない。

- [ ] 実Home routeと既存QueryClient/HTTP harnessで入口なしのREDを確認する。
- [ ] 非root/正本能力/正確target/既存主体3種・表示名fallback、5権限編集、削除と全削除禁止、継承切替の保存前確認をRED→最小実装→GREENにする。
- [ ] 同値no-op、policy revision競合、親/移動による同revision実効grant変化、閉じる/別選択/遅いread/2重送信を反例から固定する。
- [ ] 深いpayload固定、UNKNOWN後403/409と不正receipt、往復/GC、相互未確定要求、成功後可視性喪失・read reset/遅延Blob抑止・Organization保持を反例から固定する。
- [ ] focused→全GUI、schema/型/production buildを実行し、所有source/testだけを日本語commitへ保存する。

## Task 2: 既存実受入と手順

既存合成Folder fixtureを使い、新runner/fixture基盤を増やさない。UIラベルは「選択したフォルダーのアクセス設定」、確認欄「変更内容の確認」、保存「アクセス設定を保存」。詳細な行labelは意味が曖昧にならない名前をTask 1と協調する。

- [ ] 既存hosted journeyの安全なFolder区間を選び、既存主体/管理者を保つ権限変更→正規GET照合、固定payloadと子孫の継承への限定効果を確認する。API設定と他の既存fixture資格を壊さないよう既存scope内で設計する。
- [ ] 既存HTTP再起動後に同じFolderの設定を再読取する。応答喪失や自己失権の実hosted再現を加えない場合は純粋検証との差を明記し、実証したとはしない。
- [ ] runtime型・既存純粋検査・collectionを確認し、日本語操作手順と状況を同PRに含める。導入pinはmain資格完了後の同機能docs差分で追従する。
- [ ] 独立reviewで仕様/品質/認可境界と両変更の共存を確認する。未解決の重大所見を残さず同機能Draftへ保存し、remote head/tree/paths一致と同head hosted/必須CIを終端まで確認する。

## 検証の区別

単体/DOMのUNKNOWNや競合反例と、合成hostedの実操作を区別する。画像/macOS golden、本番Identity/TLS/対象PC導入、backup/restore、PostgreSQLプロセス再起動、管理operation照会によるUNKNOWN解消は今回の資格に含めない。取得不能なruntime本文の値は作らず、既存必須gateと公式step結果・固定sourceの対応で評価する。

# Organization Browser PoC の起動と確認

## 現在の範囲

2名の起動時固定の模擬ユーザーを使い、タスク一覧・詳細、privateな文案の保存、共有Document参照、提出、事務担当の引受けと提出内容の閲覧、理由付き差戻と新試行での再提出、Documentを参照する根拠・候補・人間判断の保存、選択根拠に結び付いた合成Agentの候補作成、最終事務タスクの明示的な完了、担当中タスクの保留と再開を行う。PostgreSQLを状態の正本とし、ページ再読込でも保存済み状態を取得する。未保存の入力と結果不明操作はタブ内メモリーに保持する。

これは認証システムではない。各loopbackポートへ接続できる利用者はその固定profileとして扱われる。顧客情報・秘密情報・production DBを使用しない。外部公開・production deploy・Tauri実行を含まない。

## 準備

既存Document PoCと同じLinux/Rust/Node/PostgreSQLの資格・実行権限がある環境を使う。既知のsocket/browser拒否を別ポート・別経路・別環境で迂回しない。

1. repositoryの固定toolchainを準備し、`pnpm install --frozen-lockfile --ignore-scripts` と `pnpm --filter @knowledge-platform/document-web build` を実行する
2. `cargo build --locked -p organization-server -p document-semantic-inspection-worker -p document-diff-worker` を実行する。必要な既存worker/native要件は [Document PoC運用](document-poc-runtime-v0.md) に従う
3. 新しい使い捨てPostgreSQL DB、空の専用storageディレクトリを用意する。既存Document PoC DBのpolicyを上書きしない
4. 同じDB・storage・built GUIを2profileで共有し、次の環境変数を明示する。DB URLやstorage pathをrepositoryへ保存しない

```sh
export KP_RUNTIME_MODE=organization-synthetic
export KP_ORGANIZATION_PROFILE=sales-01
export KP_DATABASE_URL='postgres://…/organization_poc'
export KP_STORAGE_ROOT='/absolute/path/to/disposable-storage'
export KP_DSI_WORKER='/absolute/path/to/document-semantic-inspection-worker'
export KP_DIFF_WORKER='/absolute/path/to/document-diff-worker'
export KP_WEB_DIST='/absolute/path/to/apps/document-web/dist'
# Document PoCが必要とする場合だけ、資格取得済みPDFiumの既存指定も設定する
# export KP_DSI_PDFIUM_RUNTIME_DIR='/absolute/path/to/qualified-pdfium'

./target/debug/organization-server migrate
./target/debug/organization-server bootstrap-poc
./target/debug/organization-server serve
```

`migrate` は明示コマンドだけでDocument既存migrationとWork専用schema/migration ledgerを処理する。通常の `serve` はmigrationもseedも実行しない。`bootstrap-poc` はDocumentの既存bootstrap portでsales-01のfixture作成権限、office-01と固定Document provider poc/poc-agentのread/readHistoryを初期化する。Agent対応前のDBを含め、既存policyが異なる場合は停止し、暗黙にgrantを追加しない。新しい合成Agent PoCでは新しい所有された使い捨てDBを用いる。

## 共有文書を用意する

タスクの入力文書はDocumentの既存APIで作成・公開する。WorkはDocumentのACLや版を変えない。既存の公開文書を2profileで読めるならそのIDを利用できる。

初めての使い捨てDBでは、salesサーバー起動後に次の合成ファイルを既存APIへ送れる。

```sh
printf '【合成データ】PoCの共有参照資料です。実在する顧客情報を含みません。\n' > /tmp/organization-poc-reference.txt
curl --fail-with-body -F 'request={"folderId":"00000000-0000-7000-8000-000000000001","title":"PoC共有参照資料","documentMetadata":{},"versionMetadata":{}};type=application/json' \
  -F 'file=@/tmp/organization-poc-reference.txt;type=text/plain' \
  http://127.0.0.1:8090/v1/documents
```

返却された `documentId` / `documentVersionId` を使い、salesの `/documents/{documentId}?view=authoring` を開いて既存の「公開」操作で公開する。作成/公開が結果不明なら、既存Documentの回復手順で同じ対象を確認してから進める。新しい文書を無条件に再作成しない。

別のshellで同じ環境変数を設定して次を実行する。

```sh
export KP_ORGANIZATION_DOCUMENT_ID='<上で公開したdocumentId>'
./target/debug/organization-server seed-work
```

このコマンドは既存Document read serviceで2profileの現在のPublishedアクセスを確かめてからWork fixtureを作る。既存Workの進捗はリセットしない。入力文書を変えるには新しい使い捨てDBを用意する。差戻対応のfixtureは新しいdefinition versionを使う。以前のforward-only定義のDBは0002 migration後も元の定義を保ち、差戻対応へ自動付替えしない。新しい差戻PoCには新しい使い捨てDBを用いる。

## 2名で一連の操作をする

- 営業: `http://127.0.0.1:8090/tasks?view=context`
- 事務: 同じDB/worker/storage/GUI設定で `KP_ORGANIZATION_PROFILE=office-01` として別processを起動し、`http://127.0.0.1:8091/tasks?view=queue`

1. 営業でタスクを選び、文案を保存する。事務の一覧にはまだタスクもprivate本文も出ない
2. 営業で「文書・比較」から共有参照資料を開く。版・改訂・比較・認可は従来のDocument機能が扱う。戻るとタスク選択を保持する
3. 営業で保存済み文案を確認して「提出」を確定する。サーバーの成功応答までPending。応答を確認できない時は同じoperation IDで結果確認する
4. 事務で再読込すると担当待ちタスクが出る。「担当を引き受ける」後、提出時点で固定された本文を読める。元のprivate artifactへの直接アクセスは許可しない
5. ページ再読込で保存済み状態を確認する。履歴・snapshot・次task・operation・必須event stagingは同一Work transactionで保存する

## 差戻して再提出する

1. 引受け済みの事務タスクで差戻理由を入力する。空白だけ・UTF-8で8KiB超の理由は使えない。確認画面のキャンセル/Escapeは送信せず、理由を保持する
2. 差戻を確定すると、事務の試行1は完了したままになる。事務画面には確定した指示を表示し、営業の新private文案へ切り替えない
3. 営業で同じタスクを再読込し、readyの試行2を引き受ける。確定理由と旧提出を読んで、新しいprivate文案を保存する。旧提出本文を上書きしない
4. 再提出すると新しいsnapshotと、同じ事務タスクの新しいready試行ができる。事務で改めて引き受けると新提出を閲覧できる。旧snapshot/理由は不変である
5. 通信結果が不明なら元のoperation IDで確認・再送する。同じtask IDでも古い試行の結果で現在の試行へ巻き戻さない

## 根拠・候補・人間判断を残す

1. 担当中のタスクでContext Surfaceの「根拠」を選ぶ。営業型・事務型のどちらでも同じ操作を使う
2. タスクに結び付いた共有Documentの現在の公開改訂と原本を選び、人間が確認した該当箇所を記載して登録する。本文は複製せず、改訂・版・原本の固定参照を保存する。人間の箇所説明は原本から検証済みの抽出結果ではなく、coverageはunknownと表示する
3. 1件以上の根拠を選び、候補の主張を登録する。候補と原本の事実は別の記録である
4. 正確な候補revisionへ「採用」「修正」「却下」の判断を残す。修正時は採用する主張を入力する。元候補・根拠・以前の判断は書き換えず、判断だけで提出や差戻は実行しない
5. 営業から提出する場合は、共有する根拠・候補・判断を明示的に選び、提出確認で参照集合を確認する。候補の根拠、判断の候補と根拠も選択集合へ含める。未選択recordは提出されない
6. 事務が引受けた後は、受領した選択recordを読み、自分の現在の試行で別の判断を残せる。事務の判断を過去の営業snapshotへ書き戻さない。新しい差戻試行のprivate記録は以前の提出と混ぜない

このPoCでは、現在の試行と受領内容を合わせた根拠・候補、および各候補の可視判断はそれぞれ16件まで。上限では新規登録を拒否し、既存記録を一覧から切り捨てない。collection APIは完全集合1page、limit省略時50・指定は16–100、cursorは未対応。

新規登録は現在公開版のAUTHORITATIVE原本だけが対象。保存後に新しい版が公開されても、過去の根拠は元の改訂・版・原本を指す。原本を開く時やWorkから返す時はDocumentの現在の権限を再確認し、別の版/ファイルへ自動置換しない。閲覧できなくなった場合は古い表示を残さず、権限/利用可否を表示する。Workの提出はDocument権限を変更しない。

入力と結果不明操作は同じ利用者・担当・タスク・試行のタブ内状態として扱う。通信結果不明時は元operation IDで確認・再送し、新しいIDで重複作成しない。タブを閉じると未保存入力は失われる。

## 合成Agentから人間判断へつなぐ

この経路のexecutorは固定規則の検証用処理で、実LLMではない。原本本文の読解・要約・事実検証はしない。Documentの現在認可は実Applicationサービスで確認するが、MCP transportは実行していない。

1. 担当中のタスクで「根拠」から少なくとも1件の参照を登録しておき、「Agent」を開く
2. 目的と、使わせる既存根拠のexact revisionを1–16件選択して依頼する。選択は権限を広げず、他のprivate文案や別タスクを自動追加しない
3. サーバーへ保存された実行IDと状態を確認する。実行中の取消は今後の処理を止めるもので、既に成功した候補を消したりworkflowを戻したりしない。通信結果が不明なら元operation IDで回復し、新しいIDを作らない。回復できたものがqueued/runningの受付記録だけなら、実行が進行中と断定せず、結果不明の表示から状態の再確認または取消を行う
4. 成功後の構造化結果から候補を確認する。作成者organization-synthetic/agent-01、依頼者、Document provider poc/poc-agent、合成実行と本文分析なしの表示を区別する。候補は通常のFindingとして保存され、chatだけには残らない
5. 「根拠」で人間が候補を採用・修正・却下する。Agentは人間判断や提出を代行しない。次担当へ共有するには既存の提出確認で根拠・候補・判断を明示選択する

同時実行はtaskごと1件、現在attemptの実行履歴は最大16件。task/attempt/責任変更、取消、原本権限の喪失後は古い実行結果を新しいcontextへ表示しない。process再起動時の未完了実行はoutcome_unknownとなり、自動再実行しない。完了済み候補の内容は書き換えない。

この追加経路の資格は[合成Agentの最新状況](../superpowers/execution/organization-synthetic-agent-slice-status.md)に記録する。以前のEvidence受入だけでAgent実動作を合格にしない。

## 最終事務タスクを完了する

1. 最終事務タスクで受領内容と判断を確認し、「完了内容を確認」を開く。定義された次担当への提出が必要な営業stepには、この操作を表示しない
2. 対象タスク・試行・現在の担当と、完了後は読取り専用になることを確認する。キャンセル/Escapeでは何も確定しない
3. 「完了を確定」で現在の試行を閉じる。新しい担当や提出snapshotは作らず、過去の提出・根拠・判断・Agent結果を保持する。履歴に完了を表示する
4. 読取りは引き続き現在の担当と原本権限で確認する。完了は非公開情報の共有を増やさない。結果不明時は同じ操作IDで確認し、新しい操作として繰り返さない

完了対応は新しいimmutable definition versionのfixtureに限定する。旧DBへWork migration0005を適用しても、既存workflowの定義と進捗を自動変更しない。以前のPoC DBでは完了操作を追加せず、新しい所有された使い捨てDBで開始する。保留/再開は後続の専用definition versionを使う。

この追加経路の資格は[完了sliceの最新状況](../superpowers/execution/organization-complete-slice-status.md)を参照する。合成Agentの既存成功を新しい完了操作の実証とは扱わない。

## 作業を保留して再開する

1. 現在担当中のタスクで「保留内容を確認」を開き、対象の試行・担当と、未保存入力を自動保存しないことを確認する。キャンセルでは何も変更しない
2. 保留を確定すると、同じ試行と担当のまま読取り専用になる。保存済み文案・根拠・判断・過去の提出は残る。未保存入力はこのタブ内だけに残り、ページを閉じると失われる
3. 再開の確認後に「再開を確定」すると、同じ試行の作業へ戻る。タブ内に残した入力も戻るが、新たな保存・提出は別の明示操作で行う
4. 保留前の古いAgent出力を新しい状態へ採用しない。再開してもAgentを自動再実行しない。過去の保存済み結果は現在権限で確認する

保留/再開は新しいfixture definition versionでのみ提供する。旧workflowの定義・担当・進捗をmigration0006で自動変更せず、新しい所有された使い捨てDBで試す。状態文字列ではなく、serverが返す現在の操作可否と定義action IDを使う。正確な同operation再送・記録の回復は保留中も読取りとして扱い、新しい変更操作とは区別する。

実受入の状況は[保留/再開sliceの最新状況](../superpowers/execution/organization-hold-resume-slice-status.md)を参照する。以前の完了操作の成功を新機能の資格へ付け替えない。

## 未対応と検証限界

- Tauri/実Windows/WebView2/native Workspace、ファイル添付、実LLM/外部model・MCP通信、検索の接続、role管理・委任は今回の最小slice外
- Work fixtureは2stepの1workflow。物理DBでは1aggregateをrow lockし、privateなschema-bound textを保存する。一般workflow designerや大規模運用を意味しない
- AuditはWork transaction内のstagingまで。別Audit pipeline配送の資格取得は主張しない
- 実PostgreSQLとbrowserの確認は、明示承認されたGitHub Actionsの使い捨て環境で行う。ローカルの既知DB/browser拒否を再試行しない。以前、純粋試験と誤認したDocument Node試験が合成loopback listenerを起動した事実は報告・終了確認済みで、実DB資格や追加実行許可を意味しない。純粋テスト/HTTP oneshot/型検査/buildの成功で実runtime合格としない

## 最小開発確認

```sh
cargo test --locked -p organization-server -p work-domain -p work-application -p work-api-http -p work-repository-postgres
cargo test --locked -p document-server --test config_identity
pnpm --filter @knowledge-platform/document-web test
pnpm --filter @knowledge-platform/document-web build
pnpm organization:api:lint
```

PostgreSQL transaction試験は既定で明示ignoreされる。実行していない試験を合格件数へ加算しない。適切な実行権限を持つ使い捨て `*_work_poc_test` DBが用意できた場合に限り、`WORK_POC_TEST_DATABASE_URL` を指定して `cargo test -p work-repository-postgres --test postgres_transaction -- --ignored` を実行する。

## Hostedでの最小実操作確認

`mise run organization:poc:runtime` は既存Document CI後段向けの単発確認である。外部DBを受け付けず、既存と同じ公式PostgreSQL一時containerを別途所有し、独立したtransaction試験用DBとbrowser用DB・storageを作る。既存固定Chromiumでsales/officeの操作を行い、2processを停止・再起動して保存状態を確認した後、所有containerを削除する。

通常CIの成功だけでなく、このOrganization専用stepのtransaction/journey/restart/persistence/shutdown成功を確認して初めて、この最小経路の実runtime検証済みとする。初回PoCの実証は[PR54](https://github.com/AIrisu-072/knowledge-platform/pull/54)のsource `44e1b412` で完了している。差戻追加経路は[PR56](https://github.com/AIrisu-072/knowledge-platform/pull/56) exact `cf28175d` で全CIと実DB/2名browser/両HTTP server再起動後復元/cleanupが成功した。根拠・候補・判断は[PR57](https://github.com/AIrisu-072/knowledge-platform/pull/57) exact `d383bacc` で実DB/2名操作/両HTTP server再起動後復元/cleanupと全CIが成功した。合成Agentは[PR60](https://github.com/AIrisu-072/knowledge-platform/pull/60) exact `48ae1bfd` で実DB/2名操作/両HTTP server再起動後復元/cleanupと全CIが成功した。最終事務の完了は別のexact-head結果で確認する。PostgreSQL processそのものの再起動は確認対象に含めていない。画像・trace・videoはoff、raw実行ログ・標準runnerの原文は一時workspace内に保持し、公開artifactは追加しない。既存の有限stage/statusと許可された操作名だけをCIへ出力する。

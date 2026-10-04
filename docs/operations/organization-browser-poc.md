# Organization Browser PoC の起動と確認

## 現在の範囲

2名の起動時固定の模擬ユーザーを使い、タスク一覧・詳細、privateな文案の保存、共有Document参照、提出、事務担当の引受けと提出内容の閲覧を行う。PostgreSQLを状態の正本とし、ページ再読込でも保存済み状態を取得する。未保存の入力と結果不明操作はタブ内メモリーに保持する。

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

`migrate` は明示コマンドだけでDocument既存migrationとWork専用schema/migration ledgerを処理する。通常の `serve` はmigrationもseedも実行しない。`bootstrap-poc` はDocumentの既存bootstrap portでsales-01のfixture作成権限とoffice-01のread/readHistoryを初期化する。既存policyが異なる場合は停止する。

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

このコマンドは既存Document read serviceで2profileの現在のPublishedアクセスを確かめてからWork fixtureを作る。既存Workの進捗はリセットしない。入力文書を変えるには新しい使い捨てDBを用意する。

## 2名で一連の操作をする

- 営業: `http://127.0.0.1:8090/tasks?view=context`
- 事務: 同じDB/worker/storage/GUI設定で `KP_ORGANIZATION_PROFILE=office-01` として別processを起動し、`http://127.0.0.1:8091/tasks?view=queue`

1. 営業でタスクを選び、文案を保存する。事務の一覧にはまだタスクもprivate本文も出ない
2. 営業で「文書・比較」から共有参照資料を開く。版・改訂・比較・認可は従来のDocument機能が扱う。戻るとタスク選択を保持する
3. 営業で保存済み文案を確認して「提出」を確定する。サーバーの成功応答までPending。応答を確認できない時は同じoperation IDで結果確認する
4. 事務で再読込すると担当待ちタスクが出る。「担当を引き受ける」後、提出時点で固定された本文を読める。元のprivate artifactへの直接アクセスは許可しない
5. ページ再読込で保存済み状態を確認する。履歴・snapshot・次task・operation・必須event stagingは同一Work transactionで保存する

## 未対応と検証限界

- Tauri/実Windows/WebView2/native Workspace、ファイル添付、差戻、Agent/Evidence/HumanDecision、検索の接続、role管理・委任は今回の最小slice外
- Work fixtureは2stepの1workflow。物理DBでは1aggregateをrow lockし、privateなschema-bound textを保存する。一般workflow designerや大規模運用を意味しない
- AuditはWork transaction内のstagingまで。別Audit pipeline配送の資格取得は主張しない
- この作業環境では実PostgreSQL・listener・browser統合を実行していない。既知拒否を再試行していない。純粋テスト/HTTP oneshot/型検査/buildの成功で実runtime合格としない

## 最小開発確認

```sh
cargo test --locked -p organization-server -p work-domain -p work-application -p work-api-http -p work-repository-postgres
cargo test --locked -p document-server --test config_identity
pnpm --filter @knowledge-platform/document-web test
pnpm --filter @knowledge-platform/document-web build
pnpm organization:api:lint
```

PostgreSQL transaction試験は既定で明示ignoreされる。実行していない試験を合格件数へ加算しない。適切な実行権限を持つ使い捨て `*_work_poc_test` DBが用意できた場合に限り、`WORK_POC_TEST_DATABASE_URL` を指定して `cargo test -p work-repository-postgres --test postgres_transaction -- --ignored` を実行する。

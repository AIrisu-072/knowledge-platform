# 文書管理基本操作・一覧・履歴参照 v0 — 設計案

- 状態: **PROPOSED / WRITTEN SPEC REVIEW PENDING**。本書の詳細設計は未承認であり、設計凍結・本番実装開始を意味しない。
- 日付: 2026-09-28 JST
- Capability: `Document Management Basics v0`
- 基準: `main@55dc3d3a430c8f36e1db8277fee15c4429258466`
- 区分: Product Capability / Architectural / 設計準備のみ
- 実行状況: `docs/superpowers/execution/document-management-basics-v0-status.md`
- 実装開始条件: 本書承認、実装計画承認、依頼者が先行させている開発ログ一元管理の完了確認、着手時の正本・CI再確認。いずれか未充足なら実装しない。

## 1. 目的と承認の範囲

既存の文書登録・原本保存・内容検査・版管理・公開・予約公開・公開終了を再実装せず、文書共通属性、フォルダ、アクセス権限、利用者別既読状態、一覧、版・操作履歴を追加する。将来のCLIとGUIが同じApplication契約を使える状態を作る。

依頼者が承認した進め方は、設計を先行し、開発ログ一元管理が完了した後に実装し、必要な業務・監査イベントは各操作と同時に実装したうえで、最後の1PRで横断検証を仕上げる、というもの。本書で提案するACL継承、予約中の変更制限、既読化条件、検索条件の具体値は、この方針承認だけで承認済みとは扱わない。

T5〜T12は `spec/data/transaction-consistency-requirements-v0.md` のtransaction分類である。既存Document Versioning実装計画のTask 5〜9とは異なる。T11/T12は独立した開発順序ではなく、必要な操作に組み込む整合性要件である。

成功条件は、対象機能の契約・保存モデル・認可・競合・イベント・受入条件が一貫し、次工程で業務判断を推測せず実装計画へ落とせることである。今回はコード、migration、依存関係、OpenAPI、既存の規範仕様を変更しない。

## 2. 正本と維持する境界

優先する正本:

- `spec/architecture/architecture-contract-v0.md`
- `spec/data/logical-data-model-v0.md`
- `spec/data/transaction-consistency-requirements-v0.md`
- `spec/operations/observability-audit-requirements-v0.md`
- `spec/operations/error-handling-resilience-requirements-v0.md`
- `spec/requirements/frontend-ux-requirements-v0.md`
- 承認済みAuthoritative Core、Document Publish、Document Semantic Inspection、Document Versioning、Document Publication EndおよびT10読み取り境界改訂1

維持する不変条件:

1. Document / DocumentVersion / FileObject / ContentItemの正本はDocument側。Searchは再生成可能な派生データのみを扱う。
2. Versionの永続状態はWORKING / PUBLISHED / WITHDRAWNのみ。現行版・過去版・公開終了表示を重複するフラグにしない。
3. 公開済み・取下げ済みVersionの内容、原本、版固有metadataを上書きしない。
4. T10後の通常参照は旧版へフォールバックしない。管理属性変更・移動・権限変更でT10を解除しない。
5. Search ExtractionとDSIを混同しない。本機能は新しい抽出器・検索エンジンを導入しない。
6. Domain / ApplicationへPostgreSQL、HTTP、Windows認証の実装詳細を持ち込まない。
7. 開発ログ一元管理は開発の先行条件であって、業務監査の保存先でも正本でもない。

基準mainには現行公開版判定・列挙、版操作台帳、公開予約、T10台帳、Domain/Audit Outboxがある。これらを流用する。既存の版管理完了タスクを未実装へ戻さない。

## 3. 対象と対象外

| 分類 | 今回の対象 |
|---|---|
| T5 | 文書共通metadataの部分変更 |
| T6 | 文書のフォルダ間移動 |
| T7 | フォルダ作成・改名・移動・階層参照 |
| T8 | 最小AccessPolicy、継承、設定変更、認可の共通境界、既存業務操作への適用 |
| T9 | 自分の文書版の既読記録・取得 |
| 参照 | 現行公開文書一覧、編集用一覧、履歴対象一覧、属性絞り込み、版一覧、文書操作履歴、版内ファイル参照 |
| 横断 | 型付き対象参照、OCC、冪等性、業務・監査Outbox記録、認可と更新の競合防止 |

対象外はHTTP/OpenAPI実装、CLI、GUI、Windows/AD/SSOの実接続、新規ユーザーマスタ、タグ・カテゴリ専用マスタ、全文・意味・横断検索、検索配送consumer、Audit Store配送worker、承認workflow、文書差分表示、T10後の再公開、原本形式移行、通常削除、物理削除、他人の既読集計、任意の一括操作である。`category`という共通属性の編集は、カテゴリマスタ管理とは区別して対象に含める。

## 4. 構成と採用案

Applicationに「認可された文書操作の入口」を置き、既存サービスと今回追加する管理・参照操作を組み合わせる。将来のtransportはこの入口だけを使い、DB・Storage・認可なしの内部サービスへ直接接続しない。

```text
将来のCLI / GUI / API
        ↓
信頼済み実行者コンテキスト
        ↓
認可された文書Application入口
        ├─ 既存の登録・版・公開・T10サービス
        ├─ metadata / folder / access-policy / read-state操作
        └─ 公開一覧 / 編集一覧 / 履歴参照
                ↓ ports
           PostgreSQL / FileStorage
```

比較した構成:

- 採用案: 認可と最小イベント型を先に共通化し、基本操作と参照を責務単位で追加する。既存コアを維持しつつ、後のCLI/GUIに権限ロジックが分散しない。
- 不採用: UI実装ごとに必要な認可・SQLを追加する。実装開始は早くても、入口ごとの認可差と重複を生む。
- 不採用: 汎用workflow・任意policy言語・分散イベント基盤を先に作る。今回の文書管理に不要な範囲が大きい。

新しい多数のcrate、汎用CQRS基盤、event sourcingへの移行は要求しない。既存の `document-domain`、`document-application`、`document-repository-postgres` 内で責務の明確なmoduleとportに分割する。具体的な分割とファイル変更手順は本書承認後の実装計画で固定する。

## 5. 実行者と権限の契約（T8）

### 5.1 認証と認可

Principalのキーは `(identity_provider, principal_id)`。表示名をキーにしない。外部identity adapterが、本人のPrincipal、issuer付きgroup/role集合、コンテキストの有効期限、実行種別を検証してApplicationへ渡す。これらを利用者のJSON、CLI引数、文書metadataから自己申告させない。

実行種別はHumanInteractive / Agent / Serviceを区別する。Agentのサービス主体と、明示的な委任がある場合の依頼者を区別し、委任によってサービス主体または依頼者の許可範囲を超えない。委任の実接続・token方式はtransportの別設計であり、本v0では未検証の委任を拒否する。

Identity Adapterの実接続は後続でもよい。試験用の明示的なPrincipal・group fixtureは利用できるが、認証なしのallow-allモードを本番の既定値にしない。providerが不明、期限切れ、membershipを検証できない場合はfail closed。外部ADの変更が反映されるまでの遅延と、Document内のpolicy更新の即時反映は別契約である。

### 5.2 最小policy model

対象はFolderまたはDocument。policy bindingごとに安定したPolicy IDとpolicy revisionを持つ。主体はissuer付きPrincipal / Group / Role参照。操作は `read`、`read_history`、`write`、`publish`、`administer` とする。操作間の暗黙の包含は設けず、必要な組合せを明示する。`administer`だけで文書内容を読めるようにしない。

v0ではallow-only方式を採用する。明示policyがなければ、文書→所属フォルダ→祖先フォルダの順で最も近い明示policyを継承する。明示policyは操作別の断片ではなくpolicy全体を置換する。明示的な空policyは拒否、継承設定への変更は親policyを使う意味とする。この二つを同一視しない。親と子のallowを足し合わせず、明示deny、条件式、ABACの任意評価は導入しない。

rootは固定IDを維持し、通常操作で移動・削除・改名しない。初回root policyは運用側が指定する信頼済みbootstrap主体を用いた専用初期化で登録し、同時に監査する。root policy未設定なら一般操作は拒否する。root policyの置換・回復は一般T8と分離し、本v0の通常機能には含めない。架空ユーザーや既存created_byから管理者を推定しない。

通常T8のpolicy置換は、変更前policyで対象への `administer` を要求する。継承へ戻す操作も同じ。自分が変更後に得る権限を根拠に変更を許可しない。policy設定の参照は `administer` が必要。policy変更は現在の全Versionに対するアクセスを変えるが、Versionの内容や過去のauditを書き換えない。

### 5.3 必要権限

| 操作 | 必要な権限・条件 |
|---|---|
| 現行公開文書・ファイル取得 | Documentのread、T10未終了、現行PUBLISHED |
| WORKING参照・編集用一覧 | Documentのread + write、T10未終了 |
| 過去版・WITHDRAWN・T10後の履歴参照 | Documentのread + read_history。残ったWORKINGを参照する場合はwriteも必要 |
| 新規文書作成 | 作成先Folderのread + write |
| Version作成・更新・rebase、T5 | Documentのread + write |
| 公開・予約・取消・取下げ・T10 | Documentのread + publish、既存業務条件 |
| 文書移動 | Documentのread + write + administer、移動元と移動先Folderのadminister |
| フォルダ作成 | 親Folderのadminister |
| フォルダ改名 | 対象Folderのadminister |
| フォルダ移動 | 対象Folderと旧親・新親のadminister。さらに§8の継承影響範囲確認 |
| 自分の既読記録 | Documentのread、HumanInteractiveによる明示的な確認、対象が現行PUBLISHED |

Folderの一覧・パンくずはそのFolderのreadで制限する。文書の明示policyにより文書だけ読めても、読めない祖先の名前や子の件数を返さない。見えている名称から権限を推測しない。

## 6. 認可・更新競合・再実行の共通規則

認可チェック後に別transactionで無条件更新する方式は認めない。Document内の権限変更とFolder階層変更を直列化するため、v0ではPostgreSQLに単一のaccess-state行を設け、単調増加する `access_revision` を保持する案とする。

- 通常の認可付き変更はaccess-stateを共有ロックし、その後で対象行をロックし、最新のpolicyと業務前提を再確認してcommitする。
- T8、文書移動、Folder移動はaccess-stateを排他ロックし、認可範囲の変更とaccess_revision加算を同一transactionで確定する。
- 共通ロック順はaccess-state → 対象Folder群のID順 → 対象Document群のID順 → policy/操作台帳。既存の公開・予約・T10も認可付き実行経路ではこの順を守る。
- ファイル検査・外部identity取得は長いDBロックの外で行う。commit前にidentityコンテキストが有効であることと、最新Document policyを再検証する。
- 純粋な読み取りは、認可・公開状態・取得を同一DB statementのsnapshotで評価する。別々の照会結果を合成して権限を判定しない。

これは低頻度の権限・構造変更を単純に安全化するv0案である。Folderを移すたびに大量のACLコピーを更新しない。将来の細粒度ロックへの置換は同じ並行テストを満たすことを条件とし、今回先回りして実装しない。

Documentのrevision、Folderのrevision、Policyのrevision、access_revisionは別の意味を持つ。T8はpolicy/access revisionを変更し、文書内容の版番号を増やさない。T5/T6はDocument revision、T7は対象Folder revisionを変更する。ReadStateはDocument revisionを変更しない。

T5〜T8の新規変更はcaller生成UUIDv7操作ID、対象、期待revision、信頼済みactor、理由、コマンドdigest、結果を永続台帳に保存する。同じID・同じコマンドは保存結果を再生し、違うコマンドはConflict。commit不明時は同じIDで照会・再試行する。JSON map順序に左右されないdigestの正規化とtest vectorを実装計画で固定する。

新しい認可付き入口では、保存結果の再生・照会にも現在の権限を確認する。権限剥奪後に古い操作IDからファイルやmetadataを得られないこと。これは保存済み成功を取り消す処理ではない。既存内部コアの再実行契約と、新しい利用者向け結果開示の認可を区別する。

新規要求が期待revision一致かつ変更なしなら、`unchanged`結果だけを台帳へ記録し、業務revision・Domain/Audit mutationイベントを追加しない。期待revision不一致は変更なしの要求でもConflict。既存成功の完全再実行は期待revision判定より先に解決するが、開示認可は省略しない。

## 7. 文書共通属性の変更（T5）

操作契約は `UpdateDocumentMetadata`。入力はDocument ID、操作ID、期待Document revision、set/unsetする共通属性、actor、理由。出力はDocument ID、操作ID、新revision、更新後共通属性、changed/unchanged、UTC日時。

v0で編集対象とする共通キーは `document_type`、`owning_department`、`category` および `extensions`。前三者は文字列、extensionsはJSON objectとする。setとunsetの同じキー指定を拒否する。既存のその他のキーは読み取り・保持し、この操作で暗黙に削除しない。既存独自キーの再分類・移行は別の明示作業とする。

title、本文、ContentItem manifest、原本参照、revision_reason、適用日、作成者・公開者、公開状態、権限情報はT5で変更しない。共通属性中の部署名・categoryからACLを自動生成しない。内容が変わる変更をmetadataに見せかけて新Version作成を回避しない。

更新は期待revision確認、metadata変更、Document revision加算、操作結果、DocumentMetadataChanged、必須監査を同一transactionに含める。過去Versionのmetadataや既読は変更しない。監査payloadは変更キー・revision・理由の許可済み範囲を基本とし、metadataの全値を無条件に複製しない。

PENDING公開予約中の実変更は拒否し、明示取消後の変更・再予約を求める。予約の期待revisionを黙って更新しない。T10終了後はread + writeに加えてread_history + administerを必要とし、管理属性だけ変更できる。T10台帳、current null、Version内容には触れない。

## 8. Folder管理・文書移動（T6/T7）

### 8.1 Folder作成・改名・階層

操作はCreateFolder / RenameFolder / MoveFolder / ListChildFolders。Folder IDは名称やpathから生成しない。pathは親関係から導出する。

名称のv0案はUnicode NFC正規化と前後空白除去、空名・制御文字・`/`・`\\`・`.`・`..`の拒否。同じ親の下で正規化後の名称重複を禁止し、大文字小文字は区別する。文字数の上限は255 Unicode scalar valuesとする。DBで同じ規則の一意性を保証する。

migrationは既存の正規化衝突・不正名を事前検査する。既存Folderを自動改名・統合しない。衝突があれば対象IDを機密情報を含めず報告して停止し、別の移行判断を求める。root以外の新規Folderは親必須、初期statusは既存ACTIVEに合わせる。archive/delete機能は追加しない。

### 8.2 文書移動

MoveDocumentはDocument ID、移動元・移動先Folder ID、期待Document revision、操作ID、actor、理由を要求する。元の所属が想定と一致すること、両Folderが利用可能なこと、§5.3の権限を確認する。所属・Document revision・access_revision・結果・DocumentMoved・必須監査を原子的に変更する。

VersionのID・番号・原本・既読は保持する。明示Document policyは保持し、継承中なら移動先のpolicyが次の認可判断から適用される。移動が閲覧範囲を変更し得ることを結果と監査に明示する。移動権限をwriteだけにしない。同一Folderへの要求は変更なし。

公開予約中の実移動は取消後に行う。T10後はread_historyも必要とし、移動によって公開を再開しない。

### 8.3 Folder移動

移動先が自分または自分の子孫なら拒否する。親存在・期待Folder revision・循環・権限を排他access guardの下で検証し、親参照・対象revision・access_revision・結果・FolderMoved・監査をcommitする。並行したA→BとB→Aの両方を成立させない。

Folder移動は、配下の継承policyの意味も変える。v0では、移動によって実効policyが変わる各Folder/Documentについて、変更前のadministerも必要とする。明示policyで継承が遮断され、実効policyが変わらない配下は影響範囲から除ける。影響範囲を全件検証できなければ、部分的に動かさず失敗させる。

配下にPENDING公開予約を持つ文書があれば、v0ではFolder移動を拒否する。改名はID・親・実効権限を変えないのでこの予約制限を適用しない。子孫Documentのrevisionを一括加算しない。構造イベントには移動Folder、旧親・新親、Folder revision、access_revisionとsubtree影響を記録し、検索側への展開・配送は別機能とする。

## 9. Policy変更と予約公開の接点（T8拡張）

権限剥奪を予約の存在で妨げてはならない。T8はPENDING予約があっても実行できる。policy変更だけで予約の期待Document revisionや内容検査証拠を書き換えない。

予約登録・手動公開・期限到達実行は同じpublish認可規則を用いる。期限到達workerは、予約台帳の依頼者Principalについて有効なgroup/role情報を再取得し、実行時のpolicyでpublish権限を再確認する。worker自身の特権で依頼者の権限不足を迂回しない。監査では依頼者とservice executorを区別する。

権限不足が確定すれば公開せず、既存の予約終端処理を使い、理由と必須監査を保存する。identity/policy取得の一時障害は公開せず、同じ予約・操作IDで再試行する。権限変更と公開が競合した場合はaccess guardで順序を確定し、剥奪commit後に認可判定する公開は拒否する。

この実行時認可は既存Versioning/T3aへの明示的な適用拡張である。従来設計で実装済みとは記載しない。本書承認後、規範T3aとVersioning設計の認可境界に追記・改訂承認を記録してからコードを変更する。既存のDSI・品質・DB時刻・冪等性の確認は維持する。

## 10. 既読・未読（T9）

ReadStateの主キーは `(identity_provider, principal_id, document_version_id)`。v0では `first_read_at` を必須とし、繰り返す閲覧で更新するlast_read_atは導入しない。最初の明示確認だけを書き込むことで、二重送信・並行実行を冪等に扱う。

`MarkVersionRead` は認証済み本人のPrincipalと明示Version IDを用いる。利用者指定の別Principalを書き込まない。HumanInteractiveの確認操作だけを受け入れ、Agent/Serviceの参照、一覧取得、プレビューの先読み、ファイル取得だけでは既読にしない。実行種別は信頼済み入口の情報であり、呼出payloadの真偽値で偽装できないこと。

登録時に対象Documentをロックし、対象が同じDocumentの現行PUBLISHEDでT10未終了であることを確認する。閲覧後に新版へ切り替わっていた場合はStaleVersionとして登録せず、新版を自動的に既読にしない。既に対象VersionのReadStateがある場合は既存結果を返すが、現在の参照権限確認は行う。

初回INSERTと `document.version.read_confirmed` の監査を同一transactionに含める。重複要求は追加イベントを生成しない。ReadState用の自然キーが冪等性を担うため、全要求に汎用操作台帳を増やさない。検索Index更新イベントは不要。新版公開後も旧版ReadStateを保持し、新版に行がないことで未読になる。

一覧・版一覧へ返すのは本人のReadStateだけ。他人の既読日時・集計・強制既読・未読戻し・過去版の新規既読確認は対象外。ReadStateは監査履歴の代替ではない。

## 11. 一覧・簡易検索

一覧を次のApplication queryに分ける。共通DTOや実装helperは共有するが、一般利用者に任意の内部scopeを自己申告させない。

| Query | 結果の範囲 |
|---|---|
| ListPublishedDocuments | read可能な現行PUBLISHEDのみ。WORKING、過去版、T10終了文書は含めない |
| ListAuthoringDocuments | read + write可能なT10未終了文書。初版WORKINGと、現行版に対する編集中Versionを区別して返す |
| ListHistoryDocuments | read + read_history可能な文書。T10終了文書も明示的な履歴文脈で返す |

共通のv0絞り込みはタイトルのliteral部分一致、Folder ID、直下/配下指定、document_type、owning_department、category、作成日時範囲。本人の未読条件は公開一覧に限定する。日時範囲はUTCの開始含む・終了含まない。タイトルは選択されたscopeのVersionのタイトルであり、公開一覧の検索にWORKINGの秘密タイトルを混ぜない。

タイトル条件はNFCと前後空白除去を行い、大文字小文字を区別する。`%`と`_`はwildcardではなく文字として扱う。任意のSQL式・JSONPath・正規表現を受け付けない。本文・ファイル内容検索、語形解析、ランキングは対象外。

既定順序はDocument作成日時降順 + Document ID降順。追加の並び順はタイトル昇順 + Document ID昇順、公開一覧に限りpublished_at降順 + Document ID降順。v0のpage sizeは既定50、上限200。cursorには形式version、query種別、sort key、filter fingerprint、principal/membership fingerprint、access_revisionを結び付ける。cursorは認可証明ではなく、各ページで改めて認可する。

認可済み集合に対してfilter・sort・limitを適用する。取得後に権限なしの行だけ取り除く実装は、件数・ページ境界の漏洩と不自然な空ページを生むため採用しない。無許可対象の総件数・facet・名前を返さない。v0では総件数を必須出力にしない。

access_revisionまたはmembershipの変化を検知したcursorは再開始を求める。ページを跨ぐ完全なsnapshotは保証せず、並行する公開・属性更新で一覧は変化し得る。返却するDocument ID / Version ID / revisionを保持し、更新操作時はOCCを行う。

公開要約はDocument ID、現行Version ID、選択Versionのtitle、許可済みFolder情報、公開日時、文書共通属性、本人既読、Document revisionを返す。Storage locator、非公開Version情報、ACL生値、他人の既読を含めない。履歴・編集用DTOでしか得られない情報を公開要約へ混ぜない。

## 12. 版・操作履歴とファイル取得

`ListDocumentVersions` は明示Document IDと履歴権限を要求し、Version番号、ID、lifecycle、現行との関係、作成/公開/取下げ日時を返す。WORKINGはwriteもある場合のみ含める。T10後も同じ条件で残存Versionを参照できるが、通常公開用のAPIへのフォールバックにしない。

`ListDocumentHistory` は版作成・更新・rebase、公開、予約・取消・終端、取下げ、T10、T5/T6の管理操作を、既存業務台帳と新しい管理操作台帳から許可済み項目だけに投影する。完全な監査調査APIではない。policyのgroup一覧、操作digest、資格情報、他人の既読を返さない。

履歴はUTCの発生日時と一意なevent/operation source keyで並べる。台帳が存在しない古い出来事の正確な時刻・実行者を推測しない。Version行に残る作成/公開情報と、詳細な操作台帳がある記録を区別し、詳細不明を明示する。Outbox配送済みレコードの削除によって通常履歴が消える構成にしない。

`GetDocumentVersion` / `ListVersionFiles` / `OpenVersionFile` はDocument・Version・ContentItem・Representationの所属関係を検証する。利用者が任意FileIdだけで原本を取得できる入口は作らない。正本/派生表現は現行ContentItemモデルに従い、旧VersionFileを第二の正本に戻さない。

通常取得は現行PUBLISHEDのみ、編集取得はWORKING + write、履歴取得はread_historyを追加で要求する。原本Storage locatorは返さず、認可後にStorage portを使う。履歴に出す旧Folder名も現在読める範囲へ制限する。

v0のファイル開示は、バイト提供前の `document.file.access_granted` を必須監査とする。これは送信完了・人間の閲読完了を意味しない。監査保存に失敗したらバイトを開示しない。通信完了/中断は将来のtransportで別記録とし、成功を先に捏造しない。

認可と公開状態の保証境界は各要求のDB判定snapshotである。権限剥奪・T10後に判定する新しい要求は拒否するが、既に許可・開始したストリームのバイトを遡って回収する保証はしない。大容量ストリームのためにDBロックを保持し続けない。

## 13. イベント・監査の型と原子性（T11/T12）

基準実装の `DomainEventRecord` はaggregate IDがDocumentIdでaggregate_typeがDocument固定、`AuditEventRecord` のresource IDもDocumentIdである。FolderやPolicyを文書IDに偽装して記録しない。

最小拡張は型付き `ResourceRef = Document | Folder | AccessPolicy` とする。既存Documentイベントの名前・ID・payloadの意味は維持する。新イベントには対象種別、正しい型のID、対象revision、操作ID、actor、UTC時刻を持たせる。Policyイベントは対象bindingとpolicy/access revisionを区別し、Document revisionとして偽装しない。Auditの新しい対象種別は保存時にも識別可能にする。

| 操作 | Domain Outbox | 必須Audit |
|---|---|---|
| T5 実変更 | DocumentMetadataChanged | document.metadata.changed |
| T6 実移動 | DocumentMoved | document.moved |
| T7 作成 | FolderCreated | folder.created |
| T7 改名 | FolderRenamed | folder.renamed |
| T7 移動 | FolderMoved（subtree影響付き） | folder.moved |
| T8 policy置換/継承変更 | AccessPolicyChanged（対象binding付き） | access_policy.changed |
| T9 初回既読確認 | なし | document.version.read_confirmed |
| ファイル開示許可 | なし | document.file.access_granted |
| 単純な一覧/metadata/履歴参照 | なし | v0では全アクセス監査にせず、本文を含まない運用観測とする |
| 重要な認可失敗 | なし | authorization.denied。本人・操作・理由分類を記録し、未確認の対象存在や本文を含めない |

変更、操作結果、必要なDomain/Audit Outboxは同一transactionでcommitする。どれか失敗したら業務変更を成功として返さない。認可拒否は業務変更transactionをcommitしないため、拒否記録は独立した監査transactionで扱う。拒否記録保存に失敗しても要求を許可へ変えず、運用警告を発する。

完全再実行、変更なし、ReadStateの重複INSERTではmutationイベントを増やさない。実行拒否と成功した変更の監査を混同しない。監査payloadのallowlist、最大長、制御文字処理を定め、理由やファイル名を含む任意入力を無制限に記録しない。監査対象と決めたイベントのsamplingは禁止する。

最後の1PRは、全対象操作のイベント対応表、既存イベント互換性、同時commit、同ID再実行、競合、途中失敗、commit不明、認可失敗時の横断検証を行う。各機能で必要な記録を未実装のまま先にマージすることは認めない。検索Index実装、配送worker、Audit Storeはこの統合PRに自動的に含めない。

## 14. 永続化と既存データ移行

追加する論理保存単位は、ReadState、Policy/bindingと主体・操作の許可集合、access-state revision/guard、新しい管理操作の永続台帳である。Folderには正規化名の一意性を追加する。Audit stagingはDocument以外の対象種別を保持できるように拡張する。既存のVersion・予約・T10・原本テーブルの意味を変更しない。

管理操作台帳にはoperation kind、型付き対象参照、actor、期待revision、command digest、changed/unchanged結果、結果revision、UTC日時を保存する。通常履歴に必要な結果は台帳に残し、配送Outboxの保持期間へ依存させない。再実行情報のTTLはv0で導入しない。

既存文書にはACLを推定付与しない。明示policyのないものは設定済みrootを含む祖先から継承する。移行を理由に全員read/writeへ開放しない。既存ReadStateはないことを前提に新規テーブルを作るが、未知の将来mainに既存実装があれば着手時に差分確認し、重複作成しない。

migration番号を今の `0005` の次と固定しない。先行タスク完了後のmainで採番し、データ衝突検査、rollback、既存コア回帰を実装計画へ含める。

## 15. エラーと利用者に見せる情報

- Validation: 不正な名前、scope/filter/cursor、キー型、操作ID。
- NotFound: 存在しない対象と、参照権限がなく存在を開示できない対象を共通化する。
- Forbidden: 既に参照可能な対象への操作権限不足。未認証なら認証境界で拒否する。
- Conflict: 期待revision不一致、同ID異要求、古いcursor/access snapshot、StaleVersion。
- BusinessRule: cycle、root保護、公開予約中の変更、公開終了後の禁止操作。
- Unavailable / CommitOutcomeUnknown / IntegrityViolation: 既存の分類と回復方針を維持する。

利用者へSQL、内部Storage path、policy内容、group一覧を漏らさない。変更結果が不明なのに失敗と断定して新しい操作IDで二重実行しない。参照不能を空の成功や部分成功へ偽装しない。

## 16. 受入条件

以下は実装後に検証する条件であり、今回実行済みのテストではない。

| ID | 条件 |
|---|---|
| DMB-01 | T5で共通属性だけが変わり、原本・Version・既読は変わらない |
| DMB-02 | T5の未知の既存キー保持、set/unset重複拒否、型検証、同値no-opが成立する |
| DMB-03 | T5/T6のPENDING予約中変更を拒否し、取消→変更→再予約で既存予約契約を守る |
| DMB-04 | T6で文書ID・Version・原本・既読を保ち、実効policyだけ正しい移動先へ切り替わる |
| DMB-05 | T7の名前衝突・root操作・存在しない親・同時相互移動cycleを拒否する |
| DMB-06 | Folder移動の継承影響対象に権限不足があれば全体rollbackし、未確認の子を移動しない |
| DMB-07 | explicit empty、inherit、nearest replacementをRust/SQL双方で同じ結果にする |
| DMB-08 | read/write/publish/administerの非暗黙包含と、本人/group issuerの区別が成立する |
| DMB-09 | policy剥奪と変更commitの順序を実DB競合試験で確定し、古い事前認可で成功しない |
| DMB-10 | T8を予約中でも実施でき、権限を失った依頼者の期限到達公開を拒否する |
| DMB-11 | identity一時障害で公開せず、同じ予約IDで安全に再試行する |
| DMB-12 | GET・先読み・Agent参照は既読化せず、本人の明示確認だけが初回ReadStateと監査を作る |
| DMB-13 | T9並行INSERTは1行・1監査、旧版の確認で新版を既読にしない |
| DMB-14 | 新版公開で旧版既読を消さず、新版だけ未読になる |
| DMB-15 | 公開一覧にWORKING・過去版・T10後文書・無許可対象・無許可件数を返さない |
| DMB-16 | filter/sort/limit前の認可、cursorのprincipal/scope/access変更拒否が成立する |
| DMB-17 | 編集・履歴の追加権限を実際に検査し、scope引数や既存内部入口で迂回できない |
| DMB-18 | FileId単独、他文書Version、他item表現、読めない祖先名による情報漏洩を拒否する |
| DMB-19 | T10後の管理変更や履歴取得が現行公開・予約・Version更新を復活させない |
| DMB-20 | 通常履歴がOutbox配送・保持整理で消えず、不明な過去情報を生成しない |
| DMB-21 | 各変更でOutbox/Audit失敗時に業務更新がrollbackし、既存イベント互換性を守る |
| DMB-22 | 完全再実行・no-op・commit不明からの回復でrevision/イベントを二重追加しない |
| DMB-23 | ファイル監査を保存できなければバイトを開示せず、access_grantedを送信完了と表現しない |
| DMB-24 | migrationの衝突停止・rollback、既存Create/Get・Versioning・DSI・T10・schedulerの回帰が成立する |
| DMB-25 | 非公開本文・資格情報・ACL・個人情報を診断ログへ無条件に記録しない |

認可はpure evaluatorの単体試験に加え、SQLの一覧判定との同一fixture比較を行う。原子性・cycle・剥奪・予約競合は実PostgreSQLで確認する。モックだけで完了にしない。性能は想定利用者と文書・Folder規模の合成データで測り、今回未測定の速度を達成済みと記載しない。

## 17. 規範仕様へ反映する差分提案

本PRでは規範仕様を先に書き換えない。本書承認後、実装計画を作成する際に、次を明示的な追記・改訂対象として追跡する。

| 規範・既存設計 | 追記する意味 |
|---|---|
| logical-data-model / transaction T5 | 共通metadataの編集対象・部分更新・no-op |
| transaction T6/T7 | 移動の権限、名前重複、cycle競合、継承影響、予約中の制限 |
| logical-data-model / transaction T8 | allow-only / nearest replacement、操作組合せ、Policy revision、access guard |
| transaction T9 / ReadState | 本人の明示確認、first_readのみ、自然キー再実行、既読監査 |
| Versioning / T3a | 期限到達時の現在認可。DSI・品質・時刻・操作IDは変更しない |
| T10読み取り境界 | 新設する認可付き履歴経路の接続。既存通常公開の禁止範囲は維持 |
| observability-audit / T11/T12 | Folder/Policy対象参照、イベント対応表、開示許可監査と高量参照の扱い |

レビューで案が変更された場合は本書・受入条件を先に修正し、既存の承認記録を上書きしない。

## 18. 次工程と実装保留

推奨する機能分割は、(A)認可契約・対象参照・原子的記録、(B)T5/T6/T7、(C)T9と認可付き一覧・履歴、(D)最後のT11/T12統合検証1PRである。T8の既存公開・予約経路への適用はAの完了条件に含める。これは設計上の依存関係であり、未作成のProduction Implementation Planを承認済みとは扱わない。

A/B/Cそれぞれの完成条件に、その操作の必要イベントと原子性試験を含める。Dまで待たない。Dで記録漏れを見つけた場合は修正するが、最初から未記録でマージするための後処理工程にはしない。

次のexact actionは、本書のwritten-spec reviewである。承認後に、実際の変更ファイル・migration・port・RED/GREEN手順・exact-head gateを固定した実装計画を作成する。計画レビューと実行方法の確認が終わっても、開発ログ一元管理の完了が確認されるまでは `READY / WAITING_FOR_LOG_CENTRALIZATION` とする。

ログ一元管理の完了は、依頼者の明示確認、または依頼者が指定した正本の完了証拠で確認する。文書管理のCI成功や時間経過を、その完了証拠に代用しない。完了を自動監視したり、後で自動実装を開始する設定は本書に含めない。

着手時はAGENTS→active→今回のstatus→承認済み設計/計画→現在main/PR/exact-head CIを読み、基準との差分を確認する。既に別作業で実装された項目を重複実装せず、未承認の差分があればその差分だけを再設計する。

# 原本構成の編集・初回複数原本登録の設計案

状態: **承認範囲で実装中。** 2026-10-08 06:23:37 UTC、親が提示した項目1（作業版だけで追加・削除・並替、最後の原本は削除不可、公開版/履歴保持）と項目7（初回複数を原子的作成、部分登録なし）の方針に対するユーザー回答「この方針で進めてください」を親から受領した。新機能の完成・試験合格を示す記録ではない。

対象は文書管理残タスクの項目1と7。既存の作業版差替え・初回単原本登録を拡張する。公開版を維持して別の作業版を編集し、既存公開transactionで切り替える方針と既読仕様を維持する。実アカウントの権限や導入先の設定を変更しない。

## 現在の境界

- `DocumentWorkingVersionEditor.tsx` と `prepareWorkingVersion` は全manifestを取得し、差替え原本だけ新bytesへ置き換える。未変更原本とRENDITIONは監査付きdownload後に再送する。結果不明時はoperationId・targetVersionId・revision・FileIds・partIds・配列順・bytes・prepared multipartを固定する。
- `CommandsVersionWrite.items` は1件以上の全manifestを受け付けるので、追加・削除・並替の書込経路は既存APIを使用できる。ただし承認済みの2026-10-05追補はこれらを明示的に対象外としている。
- `createDocument` のHTTP multipartは `request` と `file` 各1件。`CreateDocumentCommand` は単content、`DocumentService::create_document` は `InitialDocument` を作り、`AuthoritativeDocument::from_initial` は `primary` / ordinal 0 に固定する。
- 初回登録の応答・結果不明回復は Document ID / Version ID / File ID の3 IDs。PostgreSQL回復readも `primary` / ordinal 0 を照合する。複数選択だけをGUIへ追加してもAPIへ保存できない。
- `check_existing_initial_formats`、`ensure_compatible_formats`、`ensure_publish_difference` は旧新の原本を `(logicalPath, ordinal)` 一致で対応づける。順序変更はこの対応を変える。形式変更禁止を同一pathへ単純に置き換えると、現在許可される同path・異ordinalのmanifestが曖昧になる。

## 項目1: 作業版の構成編集案

既存フォーム内で原本を追加・除外・上下移動する。別版作成・保存・公開は既存の明示操作を使う。保存前に原本数と、除外する原本・補助ファイルを表示する。除外は保存まで取消可能にし、0原本の保存は許可しない。公開版や履歴への直接削除操作にはしない。

既存原本のpathは表示のみで変更しない。追加原本にはファイルとrelative logicalPathの入力を要求する。pathはNFC、相対 `/` 区切りとし、先頭 `/`、空要素、`.`、`..`、バックスラッシュ、制御文字をGUIで拒否し、backendの正規化・検証を最終判定にする。追加pathは現在フォーム内のpathと重複させない。元から存在する同path・異ordinalは勝手に修正しない。

構成変更なしの差替え保存は既存ordinalを保持する。追加・削除・上下移動のある保存のみ表示順で0から再採番する。新追加原本にはRENDITIONを付けない。既存差替えでは元のmediaTypeチェックを維持し、その原本のRENDITIONだけ除外する。除外原本に付随するRENDITIONもmanifestから除外する。保持原本・RENDITIONをすべて監査付き取得し、一つでも失敗したら保存しない。

新追加ファイルは新FileId / partId、保持原本は既存FileIdを使用する。63 binary parts、各256 MiB、JSON 1 MiB、全multipart 1 GiB、120秒の既存上限は合算する。共有FileIdを黙ってコピーせず既存のfail-closedを維持する。確定拒否後は最新版を確認して編集をやり直し、結果不明後は変更前の固定要求だけを再送する。

### 形式互換の判断

推奨は今回のGUIで既存原本の形式変更を認めず、backendの既存 `(path, ordinal)` 契約は変更しない。並替がbackendでは別anchorになることを明記する。GUIだけで全API利用者の形式変更を防ぐ保証は主張しない。既存の同anchor形式互換検査・DSI・publish差分検査は維持する。

もし「順序変更前後も同じ原本として、API全体で形式変更禁止」を要求する場合は、明示的なsource item参照と候補item対応の新契約を別途設計する必要がある。pathだけへの対応変更は既存の同path・異ordinalを安全に扱えないため採用しない。これは実装開始前の設計判断である。

## 項目7: 原子的な初回複数原本登録案

推奨は初回createへ後方互換の複数manifest形式を追加し、文書・初版・全原本・domain events・audit outboxを一つのDB transactionで作成すること。単原本create後にupdateする2操作案は、部分作成と結果不明が利用者から見えにくいため採用しない。ZIP内ファイルを原本へ推測展開する案も採用しない。

新multipart形式は `request` とbinary partsを使い、request内に `items[{logicalPath, ordinal, partId, mediaType, originalFilename}]` を置く。初回のDocument / Version / File IDsは既存どおりサーバー生成とする。legacy `file` 形式との混在、未参照parts、重複parts、part欠落、path/ordinal重複、空manifestを拒否する。単原本legacy形式は従来の `primary` / 0 へ変換して同じ保存経路へ渡す。

フォームは一つの文書名・登録先を確認し、複数ファイルを選ぶ。追加行ごとにrelative pathと順序を保存前に確認する。文書数を増やす一括登録にはしない。初期metadataは空object。登録結果はWORKINGで公開しない。公開時の既存全原本DSI検査を維持し、初回登録だけ新たに公開前検査へ置き換えない。

初回結果不明時は現在同様に再POSTしない。応答・回復情報には3 IDsに加えてordinal/path順の全初回原本File IDsを返す。legacyの `fileId` は最初の原本のIDとして残す。回復readは現在Read+Writeの認可後、全manifestを一つのsnapshotで照合し、同順のFile IDsと件数を比較する。1原本だけ一致して複数登録成功と判定しない。保存した回復receiptは生成IDsのみとし、タイトル・path・ファイルbytes・主体情報を保存しない。現在の単原本3 IDs回復はlegacy形式に限って維持する。

storageは全原本をimmutableに書き込んでからDBへ参照を確定する。確定拒否や途中storage失敗で公開済みの参照を作らない。未参照storage objectは既存のreconciliation対象にし、独自の削除処理を追加しない。DB commit結果不明ではrollback・未作成を断定しない。createの再送idempotencyを追加する場合は別のoperation ledger契約になるため、この案では現在の再POST禁止を維持する。

## 実装順序・所有ファイル

1. 設計確認後、項目1のapplication helperとフォームを拡張する。担当は `apps/document-web/src/application/document-working-version.ts`、`components/document/DocumentWorkingVersionEditor.tsx`、対応するpure tests。`DocumentDetailPage.tsx` を変更する必要はない。
2. 項目7のAPI schema・multipart parser・command/service・authoritative model・PostgreSQL初回transactionと回復readを先に実装する。SDKを正式生成し、API contractとlegacy回帰を確認する。既存の単原本fixtureを全面移行しない。
3. 複数初回登録GUIを接続する。`DocumentRegistration.tsx`、receipt helper、API adapterと対応tests。権限・登録先・遅延応答の既存世代guardを保持する。
4. 日本語操作手順、実runtime受入、再起動保持の試験を同じ機能Draft PRへまとめる。必要な独立レビューとexact-head CI、統合後CIは親が確認する。未実施の実機・DB・browser試験を合格として記録しない。

## 必須検証

項目1では2原本+各RENDITIONから、1追加、1除外、上下移動、除外取消、最後の原本除外禁止、無構成変更のordinal保持、差替えと構成変更の併用を確認する。保持ファイルのIDs/bytes/元名、対象RENDITIONだけの除外、旧公開維持を照合する。path不正・重複・parts/size上限、権限喪失、OCC競合、途中download失敗、cancel、遅延応答、画面遷移、非表示、二重送信、unknown固定payload再送を確認する。

項目7では単原本legacyと複数新形式のAPI wire、2原本以上の初版#1 WORKING、全manifestとbytes、domain/audit events、途中storage失敗、DB rollback、commit unknown、回復部分一致拒否、権限喪失を確認する。GUIは空入力・複数選択・順序・登録先変更・非表示・二重押下・unknown後再POST禁止・receiptを使う再起動回復を確認する。実受入では初回複数登録→取得→公開→再起動→全原本取得一致まで測る。

## 実装と検証の状態

承認された方針を[実装計画](../plans/2026-10-08-document-original-management-implementation.md)に具体化して進める。GUIは作業版だけで構成変更し、backendのpath+ordinal形式互換契約は維持する。実装・検証の正確な結果はexecution statusへ記録する。対象PC導入は未実施。既読を原本閲覧へ移す変更は含めない。

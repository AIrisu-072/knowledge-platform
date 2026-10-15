# 公開一覧の未読条件

## 目的と範囲

既存内部処理を通常GUIから使いたいという所有者の指示（`Sentinel_cef3cbfde698819198d9ed687212a610`）に従い、PR81統合main `0801c9864bdb7faf5fcbe7ee1062367335ee7bfb` から、既存listDocumentsのunreadOnlyを公開一覧へ接続する。Frozen管理設計§10/§11とGUI§19.2/§25を再利用する。本人・現行公開版の未読集合はserverが現在認可した結果を使い、画面側の行除外やprincipal指定、既読記録のmutationは追加しない。

## 固定する小設計

- 公開一覧の既存絞り込みformに「未読のみ」のdraft checkboxを追加する。入力中はGETせず、タイトル・属性と同じ「絞り込む」で明示適用する。offはunreadOnlyキーを削除し、on/off適用では旧cursorを破棄する。他の条件・選択は保持し、「属性の絞り込みを解除」は未読条件を維持する
- ListSearch/schemaはoptional booleanでfalse defaultを置かない。正規routerの復号後のboolean true/falseだけ受理し、AJVのcoercionで空文字・0/1/null・文字列・配列・objectを変換しない。新しいserializerやURL基盤は作らない
- serverはauthoring/historyでfalse明示も拒否する。raw URLでunread指定がある場合、view省略（既存既定のpublished）または正規のpublishedを許可し、それ以外なら理由付きでGET停止する。通常の「文書」「編集作業」navigationは既存fresh queryを使い、不正なunreadやcursorを引き継がない
- publishedのoff/false/未指定は、query keyとGETの入口でも未指定に揃える。trueだけを送信し、space文字列等をtruthinessでtrueへ変えない。旧pageSize/cursor等のfallbackでも検査済みtrueを保持する。遅れた別条件の結果を現在一覧へ混ぜない
- metadata/unread両groupの不正型・scopeを独立に確認してから、日本語error面の同アプリ内Linkで不正groupとcursorを明示解除する。片方だけ不正なら他の有効条件は保持し、両方不正なら両方を解除してerror面自身の再throwを防ぐ。既存metadataのbyte/control不正は入力保持とGET停止を維持する
- 詳細returnToは既存の同origin・正確な/documents・81920文字上限・標準parserを維持する。未読の同事前guardで理由付き停止し、catchで既定一覧GETへ落とさない。ブラウザー戻る/進むと詳細往復に適用した条件・checkbox・選択を保持する
- GETの失敗と0件を区別する。create/rename/metadata等の未知結果・receipt・離脱警告を変更しない。閲覧・絞り込み・ファイル取得だけで既読にしない。既読記録GUIは今回の対象外である

## 実装と検証

1. 既存SDK serializer・URL validator・実route DOMへ必要最小の反例をREDから追加する。正規true/false/省略、raw merge、旧条件fallback、非published false、複合不正group、入力中/submit/off、属性解除、cursor/key、詳細/履歴/選択、GET失敗/0件/遅延・未解決操作保持を確認する。基点全GUI788件を保持し型/schema/buildを確認する
2. 既存metadata-editorのjourney/persistenceの2ケース内へreadだけを追加する。通常「文書」Link、既存title/属性、未読trueの実GETと本人の未読表示、詳細往復、HTTP再起動後の保持を確認する。offは正しいcache再利用でGETしない場合があるので応答を無条件waitしない。元metadata/Minor/no-op/原本/sidecar/ケース数を維持する
3. persistedSnapshotはreadStateを保存しないため、本人とAgentのgetDocument(published).readStateを明示的に前後比較する。既存のinline readを利用し、helper用途/呼出数guardを緩めない。新MarkReadや既読済みfixtureを作らず、実の既読行除外・本人と他人の差の資格は今回追加しない。既存DB契約・合成DOM・今回runtimeの資格を分ける
4. 独立レビュー、日本語Draft、同一headの既存hosted全CI・HTTP再起動・owned cleanup・公開artifact0まで確認する。main mergeは親、実サーバー反映は所有者の手動操作

## 制約

新backend/spec/生成SDK/依存lock/CI/config/fixture/sidecar/runnerなし。日時、Folder移動/ACL、既読記録、Search/Audit/Toolbox作業、新検証基盤は追加しない。GUI内search-stateはDocument URL条件である。実通信は既存hostedのPostgreSQL18.6・固定模擬2名・Chromiumのみ。ローカルDB/socket/browser/Cargo/画像は禁止。120秒/test・retries0・画像/trace/video offと既存cleanupを維持する。golden/full visual、全headers喪失、対象PC導入/backup/restore/PostgreSQLプロセス再起動/本番認証の未資格は維持する。

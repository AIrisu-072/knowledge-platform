# 現行公開版とWORKINGの内容比較：限定GUI追加

## 目的と既存契約

既存APIを通常GUIから操作する所有者の継続指示に従う。PR93統合main `41b584ddea6c3c9ec90343f3ba98cfdac560bd24` / tree `591eb64a2d54912c2faf925b16ed70bb97265deb` が基点。既存「版・改訂」の選択WORKINGから、読取で確認した現行公開版との内容差分を表示する。正式改訂間比較と混ぜず、一つの機能PRに実装・試験・日本語手順をまとめる。

- serverの`compareVersions=available`、通常Documentの`currentVersionId`、現在読める選択WORKINGの正確なIDを使う。公開版の番号や情報は当該IDの`purpose=published`詳細、作業版は`purpose=authoring`詳細で確認する。authoring一覧はWORKINGだけなので現行版を一覧や代表版から推測しない。
- 「現行公開版とこの作業版を比較」から既存Version comparisonへ固定pair、`profile=document-diff-v0`、`projection=display`、pageSize50を送る。比較に用いた版を表示し、currentなし/WORKINGなし/能力非availableを補完しない。
- Version display応答はRevision応答と別型で、返却pair IDや正式改訂metadata snapshotを持たない。`items`・判定・coverage・未比較領域・cursor/digestをその契約のまま表示し、Partial/Unknownや空pageを差分なしと断定しない。原本確認は既存原本操作の契約を保ち、新しい変換や分析機能は足さない。
- 比較開始時の正規readと要求pairを固定し、現在Document/選択WORKINGの更新・読取拒否・失効・再取得を観測したら旧結果を隠す。server比較は指定IDの実行時snapshotを比較するため、GUIだけで「読取時からcommitまでcurrent不変」という原子的保証を作らない。戻り値を表示する際の現在read照合と固定ID表示で意味を限定する。
- page継続は同pair/digest/headerに限る。401/403/404やstale等は旧結果を隠し、明示再読取まで停止。再試行可能なtail障害だけ取得済みを保持する。close/別版/別文書/別tab後の遅い結果を再表示しない。
- 既存の更新・公開・移動・ACL変更による読取失効を新比較にも反映する。未確定mutation/Blob/Organization contextは保持し、比較readの再試行を変更要求の再送へ転換しない。
- ReadHistoryが無い編集者へ能力を推測して広げない。Folder ACL、新主体directory、Root能力不整合、既読、正式改訂単体詳細、backend/OAS/SDK新契約は今回追加しない。

## 小さい実装と検証

1. 実route/APIの既存harnessで固定pair・能力/正規read・型の違い・50+1/未比較領域・拒否/更新/遅延/close/再取得・未確定要求保持をREDにする。小さい専用read hook/表示領域と必要最小限の共通描画だけを加える。
2. 既存direct humanの`document-runtime.spec.ts`で、Version3作成後・公開前にcurrent Version2との比較を挿入する。既存single-line変更と同caseを使い、payload/内容判定・不変state・閉じて従来公開へ戻れることを確認。応答喪失proxyの許可対象POSTを広げず、新runner/context/大量fixtureを作らない。
3. 既存persistenceの文書は公開済みでWORKINGがない。必要なら新入口が出ないことを確認するが、再起動後の正のWORKING比較を実証したとはしない。実50件超・複数原本比較・画像/macOS golden等は未資格を明記する。
4. focused→全GUI/schema/型/build、runtime型・既存純粋検査・収集、独立review、同headの通常hosted必須gateを確認する。期待緩和・skip/timeout変更や新検証基盤は追加しない。

main mergeは親、実サーバーへの反映は所有者手動。PR93のsourceと失敗/成功記録を保持し、結果だけの別PRは作らない。

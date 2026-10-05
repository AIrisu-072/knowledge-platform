# 公開予約取消GUIの正規読取補修

## 根拠と境界

2026-10-05の所有者指示「作成途中の版更新と予約取り消しのAPIはすぐに作れるのであれば優先度を高くして欲しい。これ基本機能のうちに入るよね？」に従い、予約取消の小さいAPI補修とGUI接続を優先する。既存取消mutation、現在認可、OCC、同operationIdの正確な再送、公開予約の業務意味は変更しない。WORKING更新/rebaseの条件やcapabilityは対象外である。

[凍結GUI設計](2026-09-30-document-gui-integration-v0-design.md)と[凍結Versioning設計](2026-09-27-document-versioning-v0-design.md)の原文・承認blobは維持する。本書は既存予約取消を使うための追加読取fieldを記録する。所有者が以下のfield名や実装詳細を個別指定したとは扱わない。

## 最小追加

- Version detailにrequired nullable UUID `currentPublicationScheduleId` を追加する。値は将来Publish用の既存 `publish_operation_id` であり、新しい予約identityではない
- 既存の認可済み読取snapshotで、同Document・対象Version・PENDING・Versionの `scheduled_publish_at` 一致を満たす現在予約だけ返す。存在しなければnull。取消済み・実行済み・terminal・別版/別Document・projection不一致の予約を返さない
- Version一覧/Document一覧を拡張しない。新table、migration、認可方式、予約/取消/再予約のmutation規則、scheduler、履歴の保存規則を追加変更しない
- GUIはこの正規IDと既存 `cancelPublicationSchedule` capabilityが揃った場合だけ取消開始を提示する。Historyの文字列から推測しない。capabilityは表示用hintであり、現在認可・実行raceの判定は既存mutationに委ねる

## 操作と失敗

対象文書・版・予定日時を確認してから、既存契約のoperationId、publishOperationId、expectedRevisionだけを送る。理由は契約にないため新設しない。予約日時は既存詳細のAsia/Tokyo/UTC offset表示を使う。

未送信確認中に対象ID・revision・現在capabilityが変われば確定できない。二重送信を同期的に抑止する。結果不明は同operationIdと同payloadだけを再送し、新予約IDや最新revisionへ置換しない。タブ内の未解決commandはQueryClientに保持し、Back/Forward後も再開する。永続保存や別タブ回復を保証しない。タブ再読込/閉じる前に結果確認を案内する。

確定拒否は最新状態を再読取してから新しい確認へ戻る。既に結果不明となったcommandへの後続拒否は先の成功を否定しない。異なる版/Documentへ古い成功・失敗・refresh結果を適用しない。既知成功後のrefresh失敗をmutation失敗として再送しない。

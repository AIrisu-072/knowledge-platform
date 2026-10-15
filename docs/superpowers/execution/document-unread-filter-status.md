# 公開一覧の未読条件：実行状況

## 2026-10-06 03:16 UTC

- 基点はPR81統合main `0801c9864bdb7faf5fcbe7ee1062367335ee7bfb`、branch `feat/document-unread-filter-20261006`。[小計画](../plans/2026-10-06-document-unread-filter.md)を固定して実装する
- PR81属性3条件は公開head `426db3a5` / tree `794407fc` で全18checks（15成功/既存skip3）・通常CI `37405694808` 全13jobs・GUI788/39・Document18+5/属性実GET・Agent9・Organization2+2・DB36・cleanup・全4run公開artifact0成功。main push CI `37407454204` も全13jobs・属性実GET/HTTP再起動・Organization build・DB36・cleanup・公開artifact0成功
- 今回はpublished限定optional boolのGET配線。既読mutation・新backend・新fixtureを追加しない。実runtimeは既存未読文書のGET/往復/再起動とreadState不変までとし、既読行除外の新実資格とは区別する
- 03:25 UTC追補：製品 `7eb3af69`、既存runtime加算 `1621e25c` で固定。同sourceHEADの全GUI828/39・型/schema/build・両runtime型・Organization純粋28・metadata用途guard3・MCP build・collection2+2/18+5が成功。既存webpack警告3件を保持
- 単独Document modeの既存navigationはnative anchorなので、fresh hrefの確認と同アプリ内Router操作のUNKNOWN保持を区別する。ページ再読込後も要求を保持するとは主張しない
- 次の操作：日本語文書との組合せを独立レビューし、同tree Draftと既存hosted全CI・未読の実GET/readState不変/HTTP再起動・cleanup・artifact0へ進む。今回の実runtime資格は未取得

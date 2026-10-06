# 選択親への子フォルダー作成：実行状況

## 2026-10-05 22:47 UTC

- 基点main `09f79a2635b09510e2d0bdeb530ba77881a70e37`、branch `feat/document-selected-folder-create-20261005`。[小計画](../plans/2026-10-05-document-selected-folder-create.md)の限定GUI追加を開始する
- PR78のページ送りは同一headの全適用CI・実201件表示/HTTP再起動・cleanup・公開artifact0を確認してmerge済み。統合後mainのpush CI `37384154333` も全13 jobs・実201件/再起動・cleanup・公開artifact0に成功した。この資格を今回の選択親作成へ付け替えない
- 実装はsource `5286093a` で全GUI589件/35 suites、GUI型/build、両runtime型、Organization純粋26件、collection Organization2+2/Document18+5まで成功。入口不在・fresh read・非同期選択変更・共有固定要求・再選択要求の保持・旧Root要求表示・開始時page上限固定の反例をREDから確認した
- 次の操作：製品/試験と日本語文書の同じ組合せを独立レビューし、日本語Draft公開→同一headのhosted受入/全CI/cleanup/artifact0を確認する。このsliceの実hostedは未取得
- 新backend・認可推測・Search改変・画像保存・検証基盤追加は行わない。既存未資格のgolden/full visual等を維持する

## 2026-10-05 23:23 UTC — 公開前の独立レビュー

- source `5286093a` と日本語文書 `219d7096` の14ファイルを独立レビューし、確定拒否後の同じ対象の再選択回復に不具合を確認した。対象が別親へ移動した後も旧親だけを読み、共有操作が解消できない反例がREDになった
- 確定拒否の場合だけ、保存済み対象IDと一致する実再選択の読取根拠をfresh確認へ使う限定修正を進める。pending/unknownの固定要求は変更しない。Root入口の手順も、新規作成と保存済み要求の再表示を区別する
- この候補はまだ外部へ保存していない。次は修正の回帰・型/build確認と限定再レビュー、その後に日本語Draftと同一head hostedへ進む

## 2026-10-05 23:35 UTC — 限定補正の回帰確認

- 修正source `31b53ce6` は、初回の確定拒否だけに、同じ保存済み作成先IDを実ツリーで再選択した新しい読取根拠を使う。別対象・直URL・pending/unknownでは元要求を差し替えない。Root手順も新規要求と元要求の確認を区別した
- 最終全GUI597件/35 suites、型/buildは成功。実再選択後の移動・ページ範囲外・別対象・UNKNOWN固定保持・遅延見直し等の反例を追加した。修正は製品1・DOM試験1・手順1ファイルで、runtime/configは元の26件/collection資格と同bytes
- 次は限定修正とこの記録の独立再レビュー。GO確認後、修正後treeを固定して日本語Draftと同一headのhostedへ進む。旧NO-GOや以前の実通信資格を合格へ流用しない

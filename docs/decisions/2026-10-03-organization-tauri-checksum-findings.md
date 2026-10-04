# Tauriのchecksum検出に対する正確な限定除外

日付：2026-10-03 UTC。状態：**依頼者承認済み、パッチのレビューとhostedでの再検証は保留**。

## 権限と正確な範囲

質問 Sentinel_9ad83992c9c08191a0ac482c945a6f8e、
2026-10-03T10:12:48Z：

> TauriのCIで、秘密情報検査が5箇所を検出しました。確認したところ、すべて公開ライセンス文書や依存ファイルのSHA-256値で、認証情報ではありませんでした
>
> 今回のコミット・ファイル・行に固定した4つの識別子だけを、検査の限定除外へ追加してよいですか？ ファイル全体や検出ルール全体は除外しません

依頼者の返信 Sentinel_4746e698b9d88191a6303c512aaa0807、
2026-10-03T12:14 UTC：

> 除外していいです、

.gitleaksignoreへ追加するのは、不変のcommit/path/rule/lineに固定した4つのfingerprintだけです。公開PR52のcommit0802fe6de30c9491c0fe459c57b2c56641f027d9/tree17f35a26b3aea994af6860eccc296cc7ce3f88e3で追加された、独立検証済みのデータdigest5件を対象とします。既存31項目はバイト単位で不変です。.gitleaks.toml、検出ルール、workflow/taskコマンド、source/dependency/license方針、lockを変更しません。commit/file/path/rule全体の除外や、将来の検出に対する許可はありません。

正確な4項目は、`docs/research/organization-tauri-windows-inventory/runtime-terms-receipt.json` の10/21/32行に対するgeneric-api-keyの3つのfingerprintと、`docs/research/organization-tauri-windows-inventory/inventory/inactive_or_other_target-007.json` の1行目に対するgeneric-api-keyのfingerprintです。すべて上記の完全な導入commitに結び付けます。機械可読な正確な項目と回帰結果は[範囲の検証記録](https://github.com/AIrisu-072/knowledge-platform/blob/152262b5ff08e9280de0c004461da2152f0de4f2/docs/research/organization-tauri-windows-inventory/gitleaks-scope-receipt.json)にあります。fingerprintには値・列番号が含まれないため、最後の項目は、その不変の1行にあるレビュー済み2件の一致を対象とします。同じ位置でも新しいcommitは対象外です。この5件に認証情報は見つかりませんでした。

## 来歴の独立検証

固定した公式Gitleaks8.30.1の報告では、Runtimeの3件は契約レスポンスのdigestフィールドに対応します。保存したrawレスポンスと、新たに公式Microsoftから読み直したレスポンスは、再計算結果が一致します。decodeしたHTMLのdigestも一致します。これは公開文書のhashであり、API認証情報やRuntime受諾ではありません。

- [Evergreen契約レスポンス](https://developer.microsoft.com/microsoft-edge/api/eula/webview2?locale=en-us)
- [Fixed契約レスポンス](https://developer.microsoft.com/microsoft-edge/api/eula/webview2?locale=en-us&fixed=true)
- [Consumer契約レスポンス](https://developer.microsoft.com/microsoft-edge/api/eula/webview2?locale=en-us&consumer=true)

残り2件は、winapi-i686-pc-windows-gnu0.4.0とwinapi-x86_64-pc-windows-gnu0.4.0のlib/libwinapi_oemlicense.aというアーカイブ内ファイルのhashです。両方の公式registryアーカイブのSHA256がcommit済みCargo.lockと一致することを確認してから、内部ファイルのhashを再計算しました。これらは3322/3328バイトのGNU arバイナリで、選択された役割・出現がない非稼働/別targetパッケージにあります。既存のnotice_hashesフィールドはファイル名の正規表現に基づく一覧ラベルで、バイナリにライセンス表示の文章が入っている証拠ではありません。パッケージやnativeコードは実行していません。範囲を限定した[来歴の検証記録](https://github.com/AIrisu-072/knowledge-platform/blob/152262b5ff08e9280de0c004461da2152f0de4f2/docs/research/organization-tauri-windows-inventory/gitleaks-checksum-provenance.json)を参照してください。

10:16UTCの独立レビューは、この限定判断範囲のみGOとしました。5件すべてが検証済みの公開データdigestと一致しました。このレビュー自体は除外権限を与えていません。その後、上記の依頼者承認が、この4項目だけを許可しました。以前の失敗と元の公開一覧は保持し、検査を通すための履歴改変、隠蔽、改名、削除は行いません。

## 検証と残る条件

[再現用helper](https://github.com/AIrisu-072/knowledge-platform/blob/152262b5ff08e9280de0c004461da2152f0de4f2/tools/organization-tauri-qualification/verify_gitleaks_scope.py)は、公式prebuiltバイナリの正確なSHA256を要求し、走査前に31+4項目とルールのバイト不変性を検査します。self-testのソースは正確な対象taskに結び付け、実行ファイル名も検証済みバイナリと一致する必要があります。隔離したテストプロセスでは、継承されたshell起動処理や関数による上書きを使用しません。compilerやTauriコードは実行しません。正確な公開対象差分で、元の5件→承認反映後0件→元に戻して5件を検証します。また、新しいpath、および同じpath/行でも新しいcommitでのgeneric-key陰性対照、別のPAT形式の陰性対照、変更していないrepositoryのscanner self-testを検証します。出力や報告には候補値、一致値、合成fixtureの内容を表示しません。

Gitleaksは明示したignore入力に加え、対象ソースにある暗黙のignoreファイルも読みます。このため除外を元に戻す試験には、ignoreのバイトを制御した隔離Git object viewを使います。承認済みworking-tree項目を暗黙に読み続ける引数上書きでは代用しません。ソースobjectは読み取り専用で再利用し、remote取得やlazy fetchingは行いません。無関係な未公開local branchは、このviewへ取り込みません。

先に任意で試した全履歴走査は、共有partial cloneに欠けた履歴objectで止まりました。Git HTTPSの遅延取得はProxy CONNECT abortedで失敗しました。この試行は中断され、PASSには数えていません。helperはネットワークなしで対象の全履歴を再試行し、失敗・未完了を明示的に記録します。scannerエラー後に報告が空であっても成功にはしません。H1実装前には、新しいhostedの全repository履歴検査と、その後のsecurity/CI-lint gateが必須です。localの対象差分のみの結果は、他の公開refの代わりになりません。

元のPR52 CI37114872963はsecurityと集約required-checkで失敗し、それ以外のCI jobはすべて成功しました。DSI37114872993とSandbox37114873016は成功し、D2はskipされました。security taskは後続の依存・CI-lint段階より前で停止しました。このパッチには独立レビュー、親/workerによる同じDraftへのconnector公開、remote-tree一致、新しい正確なheadのhosted検査が必要です。Runtime契約の受諾・利用、Windows H1有効化、その他のnative/license/security STOPは変わりません。

最初の範囲回帰テストは、承認された4項目が存在する前に失敗しました。レビューでは、結び付けのない動的self-testをtrueへ置換しても「変更なしPASS」と誤報できることと、検証入力の改名後に隣の実行ファイルを選べることも判明しました。防止条件を加える前は、この2つの重点テストが失敗しました。現在は標準libraryの31テストすべてがPASSです。保護した検出器の再現試験、陰性対照、不変のself-testもPASSしました。local全履歴は明示的にブロックされたままで、hosted gateが必要です。このテスト修正による追加例外はありません。

## Browser PoCでの同期（2026-10-04）

公開済み152262b5の `.gitleaksignore` と同じ4 fingerprintを取り込む。既存31件・rule・対象commit/path/lineを変えず、新しい除外を追加しない。元の公開全履歴走査を再現するための同期であり、Tauri runtimeや未完成guardの利用許可ではない。

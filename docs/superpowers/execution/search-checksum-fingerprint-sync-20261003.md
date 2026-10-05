# Search：承認済みの公開 checksum 指紋4件の反映

2026-10-03。**限定した静的検査は成功。新しい公開 head の全履歴検査は確認待ちです。**

## 変更範囲

`.gitleaksignore` の既存31項目と元の4722バイト全体を変更せず、所有者が同日に承認した4つの fingerprint だけを追加します。対象は公開 commit `0802fe6de30c9491c0fe459c57b2c56641f027d9` の、正確なファイル・ルール・行に限定します。Search の別の値、新しい commit、ファイル全体、検出ルール全体を除外する変更ではありません。

| 対象ファイル | ルールと行 | 分類 |
| --- | --- | --- |
| `docs/research/organization-tauri-windows-inventory/runtime-terms-receipt.json` | `generic-api-key`、10・21・32行 | Microsoft公式WebView2ライセンス文書レスポンスのSHA-256値3件 |
| `docs/research/organization-tauri-windows-inventory/inventory/inactive_or_other_target-007.json` | `generic-api-key`、1行 | 公開WinAPI依存アーカイブ2件の、GNU形式のインポートライブラリ `lib/libwinapi_oemlicense.a` のメンバーSHA-256。同じfingerprintで2検出 |

元の承認はこの4識別子だけです。`.gitleaks.toml`、検出ルール、`mise.toml`、既存の自己検査は変更していません。EULAへの同意やRuntime実行の許可を意味しません。

## 調査で分かったこと

基準となる公開headは `0ecf486719e3c9d71242e289a7564ad6d1032b3c` です。[CI 37134510769](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37134510769) の [security job 111236185715](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37134510769/job/111236185715) は、912 commitの走査で5件を検出して失敗しました。ログには個別のmetadataがなく、artifactも0件でした。件数の一致だけで、元の5件を直接同定したとは扱いません。

同jobのcheckoutは `fetch-depth: 0` で全公開branch・tagとPRのmerge refを取得しています。実際のコマンドは `gitleaks git --redact --exit-code 1` です。[固定版8.30.1の公式ソース](https://github.com/gitleaks/gitleaks/blob/v8.30.1/sources/git.go#L93-L94)では、`--log-opts` を指定しないと `git log -p -U0 --full-history --all --diff-filter=tuxdb` を使います。そのため、別の公開branchの既知commitも検査対象になります。

現在のSearch設定と既存31例外は、既知Tauri対象commitの元の設定とバイト単位で同一でした。その同じ設定で既知Tauriの公開差分を別途走査し、5検出・承認済み4fingerprintの集合一致を確認しました。現在のSearch公開差分には検出がありません。

## 実際に行った限定検査

固定Gitleaks 8.30.1を使用し、実行体SHA-256 `88f91962aa2f93ac6ab281d553b9e125f5197bbbce38f9f2437f7299c32e5509` を再照合しました。公式配布物の検証済み実行体を再利用し、再取得・独自ビルドは行っていません。検出出力はすべてredactedです。

| 対象 | 使用例外 | 結果 |
| --- | --- | --- |
| Search公開差分 `da5e7155… → 0ecf4867…` の1commit・53ファイル | 既存31 | exit 0、検出0件 |
| 既知Tauri公開差分 `629822a4… → 0802fe6d…` の1commit | 既存31 | exit 1、検出5件、正確な4fingerprintに一致 |
| 同じTauri公開差分 | 31＋承認済み4 | exit 0、検出0件 |
| 同じTauri公開差分を旧例外で再確認 | 既存31 | exit 1、同じ5検出・4fingerprintを再現 |
| 同じSearch公開差分 | 31＋承認済み4 | exit 0、検出0件 |
| 変更していない `security:secrets:self-test` | 変更後の設定 | 想定する使い捨て検出対象1件でscanner exit 42、自己検査全体はexit 0 |

各Gitビューにはremoteを設定せず、公開refと明示した1commit範囲だけを使いました。Gitの通信を `protocol.allow=never` で禁止し、`GIT_NO_LAZY_FETCH=1` としました。これはOS全体の通信を遮断したという主張ではありません。object storeは既存の共有storeを参照しますが、private refsや未公開原本の履歴は走査していません。Git差分検査はscannerの時間上限60秒、外側のprocess上限75秒です。既存の自己検査にはscanner独自の60秒指定を追加せず、元のソースを保ったまま外側のprocess上限75秒だけを適用しました。

[機械可読の限定検査記録](search-checksum-fingerprint-sync-20261003.json)に、正確な識別子・基準SHA・設定SHA・各結果を保存しています。完全なredacted出力と実行記録は元の作業証拠として別途保持しており、ここには必要なmetadataを保存しています。秘密値は掲載していません。

## 未確認の範囲と次の確認

この比較はCIと同じ912commit全履歴の再現ではありません。公開後の新headで、通常のhosted全履歴検査が成功したことを別に確認する必要があります。元の5検出の個別reportを直接得られない限界も保持します。

Rust CIの `outbox_delivery::observe` 欠落は別の未解決事項です。G07はUnixソケットの `Operation not permitted` による実行停止を維持し、意味的なRED/GREEN、子プロセスのclaim/reap接続、G08は未達です。今回の静的検査はその制限を回避するものではなく、DB・ソケット・G07/G08の実行は行っていません。Draft、未マージ、未デプロイを維持します。

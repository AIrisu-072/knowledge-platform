意味保存の日本語訳。承認・資格の追加ではない。記載の既存ハッシュは原文/原証拠を指す。

[固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/poc-lock-metadata-recovery-20261003.md)

<a id="search-experiment-lock-metadata-recovery--2026-10-03"></a>
# Search実験用ロックのメタデータ修復記録 — 2026-10-03

> 以下の状態・次の作業は当時の記録であり、現在の実行指示ではありません。現行の停止条件・実行可否は[最新の実行状態](../../execution/search-platform-completion-program-status.md)を参照してください。

<a id="scope-and-source-identity"></a>
## 対象範囲とソースの識別情報

**対象を限定した三つの静的依存関係ゲートはPASS。PoCやプログラムの受入を主張するものではありません。** この追加作業では、[ルート依存関係のチェックポイント](dependency-metadata-recovery-20261003.md)で未解決だった三つの実験用ロックだけを修復します。変更先はSearchブランチであり、別のDocument PR #36–43は編集しません。

分離した並列作業ツリーの起点はローカルcommit `bb6cd56b1b3f587438b77527cabba1ba8da3792c`、tree `2df2784e8017f1433a2fb96d8054e4aca3478299`で、公開済みのSearch Draft [PR #40](https://github.com/AIrisu-072/knowledge-platform/pull/40)のhead `401b31047a64ed76c470477c6db15fc7e8221d2d`と一致します。既存の復旧用作業ツリーは保存しています。別のG06操作が終わった後、親担当からメタデータ処理の占有枠を与えられました。この作業ではその枠をCargoのメタデータ取得・依存解決とcargo-denyだけに使い、スキャンとメタデータ確認の終了後に解放しました。

変更するパッケージ記録は、次の各ファイルの`yoke-derive 0.8.3 → 0.8.4`と、チェックサム`33811428bee40dbceb6d545e95754741d17a6aef9a4849f0fd62e2ba4f412a78 → ec8ebde2db3681e8c9980cc27822030e68752690ddfa9473e739aeb4dbde6d71`だけです。

- `experiments/document-semantic-inspection/Cargo.lock`
- `experiments/search-http-client-poc/Cargo.lock`
- `experiments/search-discovery-poc/Cargo.lock`

ロックを解析した比較では、それぞれ337、200、376件のパッケージエントリをすべて保持しています。各ファイルで異なるのは一つのパッケージだけで、そのバージョンとチェックサムだけが異なります。基準版にこの二つの文字列置換だけを施したものとの、別途のバイト単位比較も成功しました。他のパッケージフィールド、依存関係、ソース、バージョン、機能は不変です。すべてのマニフェスト、denyポリシー、ルートのロック、ソース、テスト、ワークフロー、スキャナー例外、凍結済み契約も不変です。Active/statusファイルの更新は、意図的に親のチェックポイント担当者に任せています。

<a id="official-provenance-and-policy"></a>
## 公式の出所とポリシー

2026-10-03に新たに確認した公式の[crates.io-indexメタデータ](https://github.com/rust-lang/crates.io-index/blob/master/yo/ke/yoke-derive)はGit blob `17a2065c79a44a3d7d4f658e96e467706a957070`で、0.8.3を取り下げ済み、0.8.4を未取り下げとしています。宣言されている依存要件と機能は両者で同じです。既存のyoke 0.8.3はyoke-derive ^0.8.2を許容します。キャッシュ内の公式0.8.4アーカイブのSHA-256は`ec8ebde2db3681e8c9980cc27822030e68752690ddfa9473e739aeb4dbde6d71`で、インデックスと三つすべての候補ロックに一致します。変更していない各denyポリシーはUnicode-3.0を許可しており、置換後の最低要件であるRust 1.82はRust 1.98.1で満たせます。これは実際のパッチバージョンのソース変更であり、crateのバイト列や実行時の動作が同一だと主張するものではありません。

適用するポリシーは引き続き`spec/selection/library-tool-selection-v0.md`の§§2.1–2.2、20–21、24、`spec/architecture/dependency-rules.toml`、各実験の既存の`deny.toml`です。依存関係の本番への昇格やポリシー例外は追加しません。

<a id="exact-command-evidence"></a>
## 実際に実行したコマンドの証拠

検証時間帯は2026-10-03 05:35–05:37 UTCです。既存の検証済みツールはCargo 1.98.1（`797e8a9bc 2026-08-05`）とcargo-deny 0.20.2です。コマンドは分離した作業ツリーのルートで、次の設定を使って実行しました。

```sh
export PATH=/workspace/scratch/13897606dfde/organization-tauri-toolchain/rust/bin:/workspace/scratch/13897606dfde/organization-tauri-toolchain/deny/cargo-deny-0.20.2-x86_64-unknown-linux-musl:$PATH
export CARGO_HOME=/workspace/scratch/13897606dfde/organization-tauri-toolchain/cargo-home
export CARGO_TERM_COLOR=never
```

上に列挙した各パスから`experiments/`と`/Cargo.lock`を除いた文字列をそれぞれ`p`の値とし、次のコマンドを順に実行しました。更新前に三つすべての基準スキャンを実施し、更新ごとに正確な差分を確認してから次へ進みました。その後、三つすべての候補スキャンとメタデータ確認が完了しました。

```sh
cargo deny --manifest-path "experiments/$p/Cargo.toml" --config "experiments/$p/deny.toml" --locked check --hide-inclusion-graph
cargo update --manifest-path "experiments/$p/Cargo.toml" --offline -p yoke-derive@0.8.3 --precise 0.8.4
cargo deny --manifest-path "experiments/$p/Cargo.toml" --config "experiments/$p/deny.toml" --locked check --hide-inclusion-graph
cargo metadata --manifest-path "experiments/$p/Cargo.toml" --locked --offline --no-deps --format-version 1
```

| 実験 | 修正前のdeny | 対象限定の更新 | 候補のdeny | ロック固定のメタデータ | 残った警告 |
|---|---:|---:|---:|---:|---|
| document-semantic-inspection | exit 1 | exit 0 | exit 0 | exit 0 | 重複23件、未使用のライセンス許可2件、ovbaのライセンスフィールド欠落1件 |
| search-http-client-poc | exit 1 | exit 0 | exit 0 | exit 0 | 重複1件 |
| search-discovery-poc | exit 1 | exit 0 | exit 0 | exit 0 | 重複6件、未使用のライセンス許可1件 |

各基準スキャンのエラーは取り下げ済みのyoke-derive 0.8.3の一件だけで、最終出力は`advisories FAILED, bans ok, licenses ok, sources ok`です。各候補にはエラーがなく、最終出力は`advisories ok, bans ok, licenses ok, sources ok`です。変更前後の警告メッセージは、各メッセージの出現数も含めて一致しています。警告の抑制は行っていません。最初のシェルラッパーはルートのチェックポイントと同じexit 3を想定し、DSI基準スキャンのexit 1で停止しました。診断全文が示したのはアドバイザリーだけの失敗だったため、残りの基準スキャンはexit 1を想定して進めました。スキャナーの失敗を隠しておらず、DSI基準スキャンの再実行も不要でした。

すべてのメタデータコマンドは対応する0.0.0の実験パッケージを報告し、そのロックのSHA-256を変えていません。メタデータと依存関係のスキャンでは、プロジェクトのコンパイル、ビルドスクリプト・手続きマクロの実行、パーサー/PoC/モデル/データベース/プロセス/セキュリティの検査、別の実行用ハーネスの使用を行っていません。保存した診断ログの行末スペースとタブだけを正規化した後、`git diff --check`は成功しました。

<a id="lock-identities"></a>
## ロックの識別情報

| ロック | 修正前のSHA-256 | 候補のSHA-256 |
|---|---|---|
| document-semantic-inspection | `3e4419f233ccd59fc860d2bbbc2c1b65982f78de0a9a7dffee12194eb409bd31` | `b2cdbd9de4c26023c790a1708ff68f56f8070783caa81f89262694a50d42001f` |
| search-http-client-poc | `6eeb2322981274c59d2973eae41035db33f5cfa2ee6afe72767c1755be877cca` | `1e953759a16c58295c375976c50804430e3a985b288046bac6f9be8d2e95a905` |
| search-discovery-poc | `1c15005b6c35146a2d462737d04eb99828d72ddbc14f9b8bf8c735024e559e0d` | `2751f39b33e7c1b939a46b1136580ec72c633113158e0487b8b64796b7066b28` |

<a id="saved-diagnostic-logs"></a>
## 保存した診断ログ

次のログは出力全文と診断の順序を保持しています。リポジトリの空白ポリシーに合わせ、各行末のスペースとタブだけを除去しました。更新ログはすでに正規化済みでした。

| ログ | バイト数 | SHA-256 |
|---|---:|---|
| [poc-lock-document-semantic-inspection-after-20261003.log](poc-lock-document-semantic-inspection-after-20261003.log) | 14570 | `2fbb5a5533818532f6ad4de8ba550a1f7baee2ba6384cc8d5f2ce84c182c3ce3` |
| [poc-lock-document-semantic-inspection-before-20261003.log](poc-lock-document-semantic-inspection-before-20261003.log) | 15103 | `eb395d93fc9734b28c39739f0db80044b599345b4d03767e2113419ae9737a48` |
| [poc-lock-document-semantic-inspection-update-20261003.log](poc-lock-document-semantic-inspection-update-20261003.log) | 112 | `f01f656b0256ac4fc05481b4dc7b193c195d6c31d1cbff51911fef22c20d5618` |
| [poc-lock-search-discovery-poc-after-20261003.log](poc-lock-search-discovery-poc-after-20261003.log) | 4129 | `3bcf6eb5fc180ba48133d2a5f49d492c4fe96b92ad538448c19931faab8b941e` |
| [poc-lock-search-discovery-poc-before-20261003.log](poc-lock-search-discovery-poc-before-20261003.log) | 4654 | `39518407177026991129320288cfc0dd35a06122e72a75ed2e2d94a0106ba5ce` |
| [poc-lock-search-discovery-poc-update-20261003.log](poc-lock-search-discovery-poc-update-20261003.log) | 111 | `0111ea3643b446081e118e02ab27d857977173f35dad8017b6750f9e0f7bdfd0` |
| [poc-lock-search-http-client-poc-after-20261003.log](poc-lock-search-http-client-poc-after-20261003.log) | 614 | `71ace229d36888eb6d8d1411f73a8dba60f771a5ce6fe2b12acdfa3a6b01b2f1` |
| [poc-lock-search-http-client-poc-before-20261003.log](poc-lock-search-http-client-poc-before-20261003.log) | 1141 | `a544d93e2bb2e222b444ddfa5b433a481bf2f59c39f1346ff56f56a00726f35f` |
| [poc-lock-search-http-client-poc-update-20261003.log](poc-lock-search-http-client-poc-update-20261003.log) | 111 | `016587140e84640072a450158a0f201f1d831b146b57a7f472aa523bf08e422f` |

行末空白を正規化する前の、未正規化スキャン出力の識別情報は次のとおりです。

| 実験 / 段階 | バイト数 | SHA-256 |
|---|---:|---|
| document-semantic-inspection / 修正前 | 15149 | `ee29e19e61ba282e5735c53c6c8d7b67b7873eda2c41817dd88f9eb821c161f1` |
| document-semantic-inspection / 修正後 | 14616 | `9e6b19eb53d1dee805c260dc096ba2444280f5a204aa01224addbdbbb50a3c22` |
| search-http-client-poc / 修正前 | 1143 | `8e1036e7ccdbddec0bc3ecbd79e8c27a6ada79e25badbbfc7b758edeaa3f0164` |
| search-http-client-poc / 修正後 | 616 | `d384814fd4891687f3175a81e44f4c248f08ba97087d32d4e4c55949cccf972c` |
| search-discovery-poc / 修正前 | 4666 | `fd900ef75afb1d6ff0b0041d63c1b2adadc6d01d0b6727c7f1b195727b183175` |
| search-discovery-poc / 修正後 | 4141 | `62422a67c088ce5195e61b564578222fbec51cffe103f5f06bc7a008153fa02f` |

<a id="remaining-independent-gates-and-next-action"></a>
## 残る独立ゲートと次の作業

今回通過したのは、ローカルでデフォルト機能を使った三つの静的依存関係スキャンだけです。変更した手続きマクロのソース、各実験の動作、任意機能の組み合わせ、ホスト側のDSI資格試験、辞書・ネイティブ資産、ルートのツリー全体、P1–P7や最終受入の適合を示すものではありません。この作業では、プロジェクトの`build`、`check`、`test`、`clippy`、総合検証、パーサー、PoC、モデル、データベース、プロセス復旧、セキュリティ検査、P3資格試験を実行していません。

その時点の基準headを確認した際、PR #40は引き続きOPEN/Draftでした。Sandbox実行37099916942は成功、DSI PoC実行37099916959は失敗、CI実行37099916947は進行中でした。これらの基準headの結果は、この未公開候補の適合を示しません。欠けているobserve実装と残るP6・最終ゲートは別担当であり、この記録は別途のG05/G06検証を進めるものでも否定するものでもありません。過去の内容未特定の安全停止は未解決であり、この対象限定メタデータ修復はその再試行を許可しません。

次の作業は、三つのロックの正確な差分と、この記録・ログ一式の独立した読み取り専用レビューです。承認された場合は、その後に親担当が公開とチェックポイントの整合を行い、正確なheadの通常のホスト側CIを観測します。この作業では、リモート公開、ワークフロー起動、マージ、デプロイを行っていません。ルートの記録にある過去の三つのロックの阻害要因を更新するのは、今回の範囲内の証拠だけです。PoC全体が成功したという内容へ書き換えるものではありません。

意味保存の日本語訳。承認・資格の追加ではない。記載の既存ハッシュは原文/原証拠を指す。

[固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/dependency-metadata-recovery-20261003.md)

<a id="search-root-dependency-metadata-recovery--2026-10-03"></a>
# Searchルート依存関係メタデータの修復記録 — 2026-10-03

> 以下の状態・次の作業は当時の記録であり、現在の実行指示ではありません。現行の停止条件・実行可否は[最新の実行状態](../../execution/search-platform-completion-program-status.md)を参照してください。

<a id="scope-and-source-identity"></a>
## 対象範囲とソースの識別情報

**対象を限定した依存関係ゲートはPASS。Search P1–P7とプログラム全体の受入は引き続き未完了です。** 依頼者は2026-10-03にSearchの継続を依頼しました。独立レビューを受けた今回の修復は、その時点のDraft PR #40のhead `a945fbd32145a3109e35cb9cb056cea052698138`、tree `398edbe70174700aa55a2844dff8bd2033242582`を起点とし、`80a47960d025e4dfdea1eacade28b15d218725ff`の上に積み重ねています。以前に復元した作業ツリーは保存しており、このパッチの元にはしていません。

五つの設定・ロックファイルで変更するのは、次の内容だけです。

- 既存のローカルパス依存関係八つに、参照先パッケージと一致する`version = "0.0.0"`を追加します。内訳は`document-semantic-inspection-runner`に一つ、`search-extraction-core`に一つ、`search-extraction-runner`に三つ、`search-extraction-worker`に三つです。
- ルートの`Cargo.lock`では、`yoke-derive 0.8.3`から`0.8.4`への更新と、その公開チェックサムの更新だけを行います。558件すべてのパッケージエントリを保持し、他のパッケージフィールド、依存要件、パス、機能は変更しません。

製品ソース、テストのアサーション、マイグレーション、実行時の動作、依存関係ポリシー、スキャナーの例外、ワークフロー、凍結済みの意味的契約は変更しません。既存のexact31スキャナー例外も変更しません。

<a id="package-provenance"></a>
## パッケージの出所

新たに確認した公式の[crates.io-indexメタデータ](https://github.com/rust-lang/crates.io-index/blob/master/yo/ke/yoke-derive)のGit blobは`17a2065c79a44a3d7d4f658e96e467706a957070`で、この確認時点では`0.8.3`が取り下げ済み、`0.8.4`が未取り下げとされています。両バージョンで宣言されている依存要件は同一です。既存の`yoke 0.8.3`は`yoke-derive ^0.8.2`を許容します。置換後も許可済みのUnicode-3.0ライセンスを維持し、最低要件のRust 1.82は固定済みのRust 1.98.1ツールチェーンで満たせます。

キャッシュ内の公式`yoke-derive-0.8.4.crate`アーカイブは、新しいロックファイルにあるSHA-256 `ec8ebde2db3681e8c9980cc27822030e68752690ddfa9473e739aeb4dbde6d71`と一致します。これは実際のパッチバージョンのソース更新であり、パッケージのバイト列が不変だと主張するものではありません。

<a id="fresh-verification"></a>
## 新たに実施した検証

使用ツールはRust 1.98.1、Cargo 1.98.1、cargo-deny 0.20.2で、インストール済み・検証済みのLinuxツールチェーンを使いました。このチェックポイントでは、プロジェクトのコンパイルもテスト実行ファイルの起動も行っていません。

1. 修正前のTOML一覧確認で、バージョン未指定のローカルパス依存関係八つを再現しました。編集後に同じ確認を行うと未指定はゼロとなり、参照先パッケージのバージョンも検証できました。
2. 最初のオフラインでの対象限定解決は、復元したキャッシュに`sqlx`のインデックス項目がなかったため、ロックを変更する前に停止しました。その後、通常のcrates.ioメタデータ取得を行い、`cargo update -p yoke-derive@0.8.3 --precise 0.8.4`が完了しました。新旧ロックを解析して比較し、変更されたパッケージフィールドが意図したバージョンとチェックサムだけであることを確認しました。
3. 修復後の`cargo metadata --locked --no-deps --format-version 1`は成功しました。これはメタデータの検証であり、ビルドやテストではありません。
4. 分離したクリーンな基準作業ツリーと候補に対して、同じツールチェーンと変更していない`deny.toml`を使い、同じ依存関係ポリシーのコマンドを実行しました。

```sh
cargo deny --locked check --hide-inclusion-graph
```

- 修正前：exit **3**。四つの診断にまたがる八つのワイルドカード依存関係と、取り下げ済みの`yoke-derive 0.8.3`を検出しました。advisories/bansはFAILED、licenses/sourcesはOKでした。
- 候補：exit **0**。advisories、bans、licenses、sourcesはすべて**OK**でした。
- 両方で29件の警告が残っています。内訳はパッケージ重複26件、未使用のライセンス許可二件、`ovba 0.7.1`のライセンスフィールド欠落一件です。警告やdenyポリシーの抑制は行っていません。

保存したコマンド出力は次のとおりです。リポジトリの空白ポリシーに合わせ、各行末のスペースとタブだけを除去しています。診断文と順序は変更していません。

- [修正前のログ](dependency-metadata-before-20261003.log)：18,346 bytes、SHA-256 `027766ab5cf96740f028be5d157ae4e45283cc2c475a8b36902ecd0a999ba37c`。正規化前の出力は18,398 bytes、SHA-256 `25244ded24b97038ff623a8fffdd440776f3eccbfc738645e45ef060ac5a7c2b`です。
- [候補のログ](dependency-metadata-after-20261003.log)：15,286 bytes、SHA-256 `5fbe6264fbc595a91303aac2d2c9fadb6463b6cb6306bd82f2bf08d2d339e59c`。正規化前の出力は15,338 bytes、SHA-256 `9a4a605e92a6cedb0023b5165cd03339ecefa58cbe1b611194bbad72045df221`です。

五ファイルの差分は、パッケージの同一性、正確なロック差分、公式の出所、ライセンスとツールチェーンの互換性、ポリシー不変について、独立した読み取り専用レビューでGOを得ました。`git diff --check`も成功しました。これらの依存関係チェックでは、パッケージのビルドスクリプト、手続きマクロ、実行時の動作を動かしていません。

<a id="remaining-gates-and-next-action"></a>
## 未達のゲートと次の作業

別管理の三つの実験用ロックファイルには、引き続き`yoke-derive 0.8.3`が固定されています。対象は`experiments/document-semantic-inspection/Cargo.lock`、`experiments/search-http-client-poc/Cargo.lock`、`experiments/search-discovery-poc/Cargo.lock`です。今回のルートのゲート通過は、これらの依存関係ゲートやホスト側のDSI PoCワークフローの合格を意味**しません**。

`outbox_delivery::observe`の実装は依然として欠けています。G05/G06の新たなランナー検証、G07/G08の実行、すべての機能受入と最終受入は未達です。過去のP1/P2/P7の記録で、現在のツリー全体が適合したことにはなりません。以前に安全上の理由で停止した内容未特定の操作は再試行していません。今回のメタデータ修復は、その過去の保留が解消したと宣言するものでも、パーサー、データベース、モデル、プロセス復旧、セキュリティ検査、P3資格試験の操作を許可するものでもありません。

次の作業は、独立レビュー済みのこの対象限定チェックポイントだけをDraft PR #40で公開し、正確なheadに対する通常のCIを確認することです。次の明示的に範囲を限定したP6ゲートは、実行前に凍結済みのG05 → G06 → G07 → G08の順序に照らしてレビューします。別管理の実験用ロックの修復も、範囲を限定して独立に検証します。マージやデプロイは行いません。

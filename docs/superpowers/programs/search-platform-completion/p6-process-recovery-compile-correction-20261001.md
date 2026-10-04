意味保存の日本語訳。承認・資格の追加ではない。記載の既存ハッシュは原文/原証拠を指す。

[固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-process-recovery-compile-correction-20261001.md)

<a id="p6-process-recovery-draft--compile-only-correction-2026-10-01"></a>
# P6プロセス復旧の下書き — コンパイルのみの修正記録、2026-10-01

> 以下の状態・次の作業は当時の記録であり、現在の実行指示ではありません。現行の停止条件・実行可否は[最新の実行状態](../../execution/search-platform-completion-program-status.md)を参照してください。

<a id="scope-and-independent-review"></a>
## 対象範囲と独立レビュー

**WIP。P1–P7とプログラム全体の受入は引き続き未完了です。** 保存済みのP6テスト下書きに対する一行の型注釈と、このチェックポイントの追加だけです。本番実装、依存関係、スキーマ、フィクスチャ、スキャナーポリシー、パーサー、P3ワークフロー、実行時の動作は変更しません。

親はDraft PR #40のhead `99c7aca2e7ae3f1f60fa3b948b419a2ae5fc749a`です。`crates/outbox-delivery/tests/process_recovery.rs:306`の`previous.get("lease_token")`を`previous.get::<Uuid, _>("lease_token")`に変更します。独立した静的レビューでは、マイグレーション0009が当該列をUUIDと宣言していること、outboxモデルがUuidを使っていること、同じアサーション内の現在行のデコーダーがすでにUuidを要求していること、このファイルが`uuid::Uuid`をインポートしていることを確認しました。この注釈は、アサーションやデータベース操作を変えずにSQLxのDecode/PartialEq型推論を解決します。

レビューしたパッチは、作成元の作業ツリーと`99c7aca2e7ae3f1f60fa3b948b419a2ae5fc749a`の唯一の差分と完全に一致します。記録したREDとGREENの間に、他のソース変更はありません。

- 元ソースのGit blob：`3cdfdb1373f61e1456995a5152389ebb42347585`
- 修正後ソースのGit blob：`f930c0693cd881e2f95c9b9d234e602e1e075064`
- 元ソースのSHA-256：`577adec41f3c1482892c3142d42af823b670886d81b6547514e19d86f4ce24d4`
- 修正後ソースのSHA-256：`c6c513f61f916e3bc46474b348a0d877e00ae36f1067a1ebb397a33e009b65ba`
- パッチのSHA-256：`81c9e2a56ec7d857cc13ab158508e4f5f4abd7dc1a720900f47af0820abe4eda`

<a id="bounded-verification-evidence"></a>
## 範囲を限定した検証の証拠

作成元の作業ツリーでは、Rust/Cargo 1.98.1を、`CARGO_PROFILE_DEV_DEBUG=0`、`CARGO_INCREMENTAL=0`、分離したターゲットディレクトリ、固定済みツールチェーンとCargoホームで実行しました。一行の編集前後で同じコマンドを実行しています。

```sh
cargo check --locked -j 2 -p outbox-delivery --test process_recovery
```

- RED：exit 101、306行目でE0283の診断が二件
- GREEN：exit 0、対象を限定したdevプロファイルのチェックが0.52sで完了
- 作成者は`rustfmt --edition 2024 --check crates/outbox-delivery/tests/process_recovery.rs`と`git diff --check`の成功も記録
- 独立レビュアーは、保存済みのRED/GREEN出力、正確なパッチ、ソースのバイト列、スキーマとモデル、隣接するデコーダーを確認し、静的な差分チェックを再実施しました。Cargoは再実行して**いません**
- RED出力のSHA-256：`4cf1eb8f912ba201811ffe307ffc273aa21b81be152d3d399d073af70093f6a9`（10,340 bytes）
- GREEN出力のSHA-256：`0ab5b3219badc4ece95cddaa0464a685199c0069ca7829a0d09c2397cb197107`（178 bytes）
- それ以前のオフライン基準確認は、arc-swapのキャッシュ欠落によりコンパイル前に停止しました。その後、Cargoマニフェストとロックを変更せず、ロックに従った通常の依存関係取得が完了しました

テスト、DBサーバー、ワーカー子プロセスのシナリオ、パーサー・セキュリティテスト、ベンチマーク、モデル実行、P3資格試験は行っていません。対象を限定したコンパイルは、G07の実行、G08の実装、パッケージ全体のコンパイルやテスト、プログラム全体の資格試験には相当しません。

<a id="unresolved-gates-and-current-ci-boundary"></a>
## 未解決のゲートと当時のCIの範囲

元のhead `99c7aca2e7ae3f1f60fa3b948b419a2ae5fc749a`のホスト側CIは、失敗を含む状態で完了しました。失敗内容は、このテストのE0283、`outbox_delivery::observe`が存在しないことによる`observability_security.rs:14`のE0432、セキュリティスキャンの指摘、DSI PoCで使う取り下げ済みの`yoke-derive 0.8.3`に関するアドバイザリーです。SQLxはスキップされ、Rustテストはコンパイル段階で停止し、実行には至っていません。Sandbox、ポリシー、コンテナ/SBOM、macOS DSI一致性の両構成、移植性の成功は、その以前のheadに限られます。

この修正で対処するのは、明記したE0283だけです。observe実装の欠落、G07/G08の実行時証拠、P1 Office v2、P7の保留中の登録、他のすべての機能受入、新しい正確なheadのホスト側ゲートは未達です。Gitleaksの誤検知の切り分けでは、検証済みソースのチェックサム28件と、合成ローカルフィクスチャの一致三件を特定しました。正確なフィンガープリントに限定する提案は未適用です。より広い実装作業に対する安全上の保留と、独立レビューの要件は引き続き有効です。

初回の`draft-source-manifest-20261001.json`は、初めて公開した`99c7aca2e7ae3f1f60fa3b948b419a2ae5fc749a`のスナップショットについて、変更不可の証拠として保持します。この記録はその後のソース差分を記述するものです。古いソースハッシュを新しいファイルのハッシュとして扱わないでください。除外したP3のホスト側ワークフロー・補助スクリプト・テスト・提案と、生成物・キャッシュ・モデル実行の成果物は、PRの外でローカルに保存しています。

**次に行う具体的な作業：** fast-forward後のDraftのheadを確認し、その正確な新headについて、既存の通常のホスト側CIを観測します。以前の成功が引き継がれるとは主張しません。このコンパイルのみの記録を根拠に、修復、パーサー/P3の実行、より広い実装作業、スキャナーの抑制を始めてはいけません。マージやデプロイは行いません。

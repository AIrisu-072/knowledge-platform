# Documentプレビュー用ツール変更02の承認記録

日付: 2026-10-03 UTC。状態: **依頼者承認済み。レビュー済み範囲でソース実装・純粋なTDDを進行中**。

## 正確な承認根拠

質問 Sentinel_2c40a3a826448191bc60f1594f50144d,
2026-10-03T14:42:21Z:

> 脆弱性のある開発用サーバーを外し、画面の本番ビルドを表示する小さなプレビューサーバーへ置き換える案が、設計レビューを通過しました
>
> 利用者向けの画面やAPIは維持しますが、開発中の自動更新とエラー表示はなくなり、手動の再ビルド・再起動になります。この変更で進めてよいですか？

依頼者の返信 Sentinel_8d95d7ba414c81918ea4424e81a4a13d,
2026-10-03T14:50 UTC:

> 進めて問題ないです

## レビューされた正確な範囲

この承認は、[ツール変更02](../superpowers/specs/2026-10-03-document-preview-tooling-amendment-02.md)
と[手順計画](../superpowers/plans/2026-10-03-document-preview-tooling-amendment-02.md)に記した開発時の更新操作の変更を許可する。
文書単独の独立レビューGOはlocal80de02121f0487b0a76adc7fa5f23b4cf1bfec34、
treef94d0ccb65c3f446e2a6b1393c48c0f4199ca66eに結び付く。
元の提案に残る「依頼者承認待ち」は当時の記録であり、そこで定めたsecurity・動作・testの境界は維持する。

対応する固定版pnpmの正規の依存解決によって本当に削除できた場合に限り、脆弱な開発サーバーの依存枝だけを取り除く。
同じReactの本番Webpackビルド、Document GUI/APIの動作、router fallback、全ての機能・mockのassertion、実際のcomposition-root acceptanceを維持する。
別途検証するNode組込み機能だけのloopback・固定distプレビューを実装し、HMR・自動更新・Webpackコンパイルエラーのoverlayを、手動の再ビルド・再起動へ置き換える。
アプリ自身のエラー・競合・部分結果の表示は変更しない。
新たな第三者packageへの置換、fork、scanner除外、アドバイザリ例外は許可されない。

既存のmock screenshot比較7件は引き続きdarwin限定とし、baselineとplatform条件を変更しない。
互換性と権限が確認できるcloud-macOSでのvisual gateは未解決である。
Linuxの機能テスト成功では、このgateを完了できない。
baselineの再生成、新たなscreenshotアップロード、依頼者のMac利用は許可されていない。

## 実行と公開の境界

独立branch fix/document-production-previewで、ソース実装・純粋なTDD・範囲を限定した依存検証を進めてよい。
サーバー・browser・runtimeを動かす前に、その具体的な実行環境の権限を確認する。
別件のSearch Unix-socket EPERMや、以前のcloud-localhost browser制限を再試行・別経路で回避してはならない。
独自ライセンスのRuntime受諾、Windows/H1有効化、本番deploy、mergeは許可されない。

不変の10文書checkpointとbundleは、進捗文書全体の公開確認が別途保留されていた時点の記録として、ローカルに保持する。
保留対象のpacketを書き換えたり、拒否されたstatusを別branch/経路で公開したりしない。
この承認は、その公開確認と、期限付きのTauri2-package検証データ用アドバイザリ規則の双方とは別である。
新しいsourceとgate結果は、独立レビューと正確なheadの検証を経るまで、成功と扱わない。

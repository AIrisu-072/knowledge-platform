# 既存APIによる版の取下げ・文書の公開終了GUI

## 依頼の範囲と小さい画面変更

所有者の2026-10-05の指示「GUI v0ではまずは画面操作のみで大半のことができるように内部処理があるものを実装して使えるようにして欲しい。新規作成する必要があるものは後回しでいいです。」の範囲で、既存の版一覧へ二つの操作を追加する。取下げは今回、通常公開画面で取得できる現行公開版に限定する。内部の細部への個別事前承認を意味しない。

公開時の基点は、受入済みPR69をmergeしたmain `e9c7f7737f1ddac676c3880475b83fb3cd7135c7` / tree `9085ce0db85857df50ad7d80b6db71d16477df40`。ローカル作業基点の初回登録GUI `2470d8cdb5386320301e001dc81cb3daff3ea28f` とは同一treeであり、公開commitのparentをmainへ合わせる。凍結[GUI設計](../specs/2026-09-30-document-gui-integration-v0-design.md)、[Versioning設計](../specs/2026-09-27-document-versioning-v0-design.md)、[公開終了設計](../specs/2026-09-27-document-publication-end-v0-design.md)と承認記録を維持する。今回のUI追補は既存APIの配線だけであり、正本・認可・状態遷移を変更しない。

- 「選択版を取下げ」: 選択版のwithdraw capabilityだけに従う。理由を入力し、現行版の取下げでは直前の安全な公開版だけが復帰し得ること、復帰できなければ公開版が無くなることを確認する。復帰の可否はAPI結果だけで表示する。過去版の操作選択は今回追加しない
- 「公開を終了」: DocumentのendPublication capabilityとAPIの期待現行Version IDを使用する。理由を入力し、通常公開の終了、予約の無効化、原本・過去版の保持、現行v0で通常操作から再公開できない影響を確認する
- 共通: 既存React Aria確認dialog・Query・typed clientを使用する。空白理由を拒否し、取消は送信せず、二重押下を抑止する。送信時のoperationId、対象、期待revision、理由を固定し、結果不明は同じ要求だけを再送する
- 遷移: 同じアプリ起動中の戻る/進む・別文書/版への移動でも未解決要求を保持し、遅延応答を別対象へ適用しない。未解決要求中の再読込/タブを閉じる操作には警告する。永続保存や新しい回復APIは追加しない
- 競合・権限失効: 409/401/403/404/422は表示して最新状態を確認し、明示的にやり直す。すでに結果不明だった要求は後続の拒否でも未実行と断定せず保持する。成功後はdetail、Version、Revision、history、files、一覧・比較のqueryを無効化する

## 後回しにするもの

- 過去版を選ぶ取下げ導線: 既存authoring purposeはWORKINGだけ、published purposeは現行公開版だけを返す。過去版はhistory purposeとreadHistory権限が必要。暗黙にreadの用途を広げず、明示的な履歴表示切替を別のUI追補とする

- 予約取消: 既存adapterはあるが、取消に必要なpublishOperationIdをDocument/Version readが返さない。History.sourceKeyは汎用文字列であり、現行PENDING予約を指す契約がない。履歴から推測・再構成せずread契約の補完待ちとする
- WORKING更新/rebase: 初回WORKINGは現在公開版が無く既存mutationが拒否する。edit/rebase capabilityにも実mutation条件との不整合がある。さらにPUTはmanifest全置換なので、単一Fileフォームの転用は追加原本やrenditionを失う。backend契約の整理または完全manifest UIを別途扱う
- 新backend/API、Organization添付、本番Identity、外部モデル、Search/Audit/Toolbox停止作業は対象外

## 実装と検証順

1. 純粋GUIに確認/取消、capability、理由、二重押下、同payload再送、409/権限失効、結果不明・遅延応答・戻る/進むを先にREDとして追加する
2. 既存typed API adapterと小さい確認部品でGREENにする。依存・lock・backendを変更しない
3. 既存hosted受入へ画像/trace/videoを記録しない専用specを追加し、合成文書の取下げ・公開終了と再起動後の保持を同じ固定2名/使い捨てPostgreSQL/Chromiumで検証する
4. 全GUI・型・production build・collection-only・独立レビューを行い、日本語packetで親へ引き渡す。親がDraft公開後のexact-head CIとhosted実受入を確認する

ローカルDB/socket/listener/browserは起動しない。実サーバー反映は所有者の手動操作とする。

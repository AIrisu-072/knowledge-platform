# 公開予約取消の独立レビュー

## 2026-10-05 05:06 UTC — 限定GO

- 対象: 既存VersionDetailのnullable currentPublicationScheduleId、snapshot照会、生成DTO/SDK、確認付き取消GUI、既存runnerの専用非記録受入
- 判定: 未解消Critical / Importantなし。実DB/実browser/exact-head CIの合格を示すものではない
- 最終component blob: `750c31fe0bb5be067b9eabbff05b05799ec01379`。route blob: `2017fb67ee8a75482d68f62b928f805984d09d19`

## 見つかった問題と修正

1. Important: 取消成功後、再読取で取消ボタンが消えるとkeyboard focusがbodyへ落ちた。独立DOMで実際のclickも含めREDを再現した
2. 同contextで開いている確認だけを成功時に閉じ、取消triggerまたは選択tabへfocusを戻す。generationと取消可能なrequestAnimationFrameにより、古い画面の応答から新しい画面へfocusを移さない
3. 同じ反例はGREEN。製品回帰にも成功後focus試験を追加し、取消GUI23/23成功

## 独立確認

- 追加反例5/5成功: 成功後focus、拒否後の新規確認、refresh失敗保持、別Documentへ移動した古refreshの隔離、成功後の離脱警告解除
- 正規readは既存REPEATABLE READ内の同Document/Version/PENDING/予定時刻一致に限定する。DB schema、既存mutation/認可/capability条件は不変
- 初回送信直前のcache同期検査、未知結果の固定ID/payload再送、unknown後の403保持、対象scopeを確認
- main `5d262557` とのroute/runtime設定/有限診断の統合を限定レビュー。既存取下げ・公開終了と今回予約取消の両受入を残す
- diff検査成功。独立担当のAPI/診断実行1回は取消され結果を得ていないため、独立PASS証拠には含めない。親担当によるAPI16件とruntime純粋116件の結果は別の検証記録とする

実DB/browserはローカルで実行せず、公開後の既存hosted受入へ引き渡す。新しい検証環境や公開artifactを追加しない。

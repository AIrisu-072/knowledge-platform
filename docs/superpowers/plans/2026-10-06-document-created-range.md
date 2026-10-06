# 文書作成日時の範囲を通常GUIへ接続する

## 目的・基点・承認範囲

既存内部処理を画面から使えるようにする所有者の指示（`Sentinel_cef3cbfde698819198d9ed687212a610`）に沿い、main `dc04ba4af896fd8c5f2e23438cf264e054341f81` から既存listDocumentsのcreatedFrom / createdBeforeだけを接続する。Frozen管理設計§11の開始含む・終了含まないと、Frozen GUI§19.2/§25のURL正本・詳細往復を維持する。公開予約にある日時入力とJST変換helperを再利用する限定UI案を2026-10-06 05:57 UTCに確認済み。新backendや日時のAPI構文規則を追加しない。

## 限定UI案

- 全3一覧で「作成日時の開始（含む）」「作成日時の終了（含まない）」を表示し、既存のカレンダー・時刻入力 `datetime-local` と `jstDateTimeLocalToUtc` を使う。JST / UTC+09:00・分単位・文書自体の作成日時であることを明示する。公開日時や一覧の表示日時とは区別する。片側だけ指定でき、既存「絞り込む」で明示適用する
- 通常の入力は既存helperでUTCへ変換する。URLの原文がこの分入力から完全に同じ文字列へ戻る場合だけ編集欄へ展開する。秒・小数・別offsetなどを分へ無言で丸めない。日時構文や範囲の最終検査は既存APIへ委譲し、独自RFC3339 parser・日付ライブラリを作らない
- 分入力へ正確に戻せない条件は原文保持して適用中の値を読取表示する。通常利用者へRFC3339の手入力を要求しない。各端は保持・指定し直し・解除の3意図だけで管理し、「日時を指定し直す」でカレンダーへ明示置換を開始する。指定し直し中の空欄・不正入力では適用せず、取消はその端の元原文、専用解除はその端だけを外す。反対側の条件を消さない。「日時の条件を解除」は両端だけを明示解除する
- 他条件の適用、並替、ページ送り、詳細往復は日時原文を維持する。URLの両端を一組の基準とし、変更されたrenderでは旧draftをeffect同期待ちの間も無効化する。遅いreadや以前の入力を新しい条件へ混ぜない

## URL・GET・失敗回復

- 2項目はoptional string。AJV前に非string・制御文字・孤立surrogateを止め、各128 UTF-8 bytesのGUI転送上限を理由付きで守る。切詰め・trim・timezone補完はしない。空文字だけを未指定にし、Routerのraw merge後もGET/keyで省略する。上限はAPIの日時構文仕様とは別のGUI境界である
- 旧条件のfallbackで日時を捨てて広いGETへ落とさない。metadata/未読/日時の不正groupを独立に解除し、別groupの有効な原文を保持する。日時型等が不正なら両端とcursorだけを明示解除する
- 条件適用・日時解除はcursorを破棄し、他の条件・selection・panelを保持する。属性解除は日時を維持する。詳細returnToの同origin・正確なpath・標準parser・81920文字上限を維持し、実URL超過でも条件を失った既定GETへ落とさない
- API422は0件・通信失敗と区別し、URLと入力を保持して指定し直し/解除で回復する。既存固定ApiFeedbackに従い、公開されない内部原因から不正な片側を推測しない。既存認可・Query retry・mutation store・UNKNOWN要求は変更しない

## 実装・検証の小計画

1. 既存pure/SDK/実route DOMへREDから追加する。JST変換・境界ラベル、原文保持・精密条件の置換/取消/未入力停止、URL変更中のdraft、raw merge/fallback、128-byte上限・returnTo上界、複合解除、422/0件/遅延read、未解決mutation保持を確認する。基点全GUI828件と型/schema/buildを維持する
2. 既存metadata-editorのjourney/persistenceだけへreadを追加する。通常カレンダー入力の実GET、既存Document.createdAtの精密原文保持、開始包含/終了除外、詳細往復・再起動を検査する。createdAt欠落を現在時刻で補完しない。元のmetadata/原本/Minor/no-op/readState非変更を保持し、新fixture・case・sidecar・runnerを作らない
3. 日本語操作説明・資格・独立レビューを同じ機能PRへまとめ、同head既存hosted全CI・再起動・owned cleanup・公開artifact0を確認する。結果だけの別PRや統合専用PRは作らず、親がmainへ統合しmain自身のCIも確認する

## 維持する制約

新backend/OpenAPI/生成SDK/依存lock/CI・新検証基盤・Folder移動/ACL/既読mutation・Search/Audit/Toolbox作業なし。実通信は既存hosted Ubuntu/PostgreSQL18.6/合成profile/Chromiumだけ。ローカルDB/socket/browser/Cargo・画像・trace/videoなし。timeout/retry/skip/goldenを緩めない。画像資格、全status/headers喪失、対象PC導入/backup/restore/PostgreSQLプロセス再起動/本番認証の未資格を保持する。実サーバー反映は所有者の手動操作である。

# 選択したフォルダー名の変更

## 目的と範囲

既存内部処理を通常GUIから使いたいという所有者の指示（`Sentinel_cef3cbfde698819198d9ed687212a610`）に従い、既存PATCH改名APIをフォルダーツリーの選択対象へ接続する。基点はPR79統合main `5d9e3c46c5b8ed1b5dd8cbe32edefb46482f787a`。新backend・非root detail API・権限規則・依存・汎用mutation基盤は追加しない。Rootの通常改名不可は現在のserver capabilityとmutationが既に整合している。

## 固定する設計

- ツリーで選択した非root対象のID、行を載せた親ID、既取得ページ数を使う。既存`readSelectedFolder`で先頭から新しいopaque cursorだけを追い、既取得範囲の現在行と、対象自身のrenameFolder capabilityを得る。直URL・未発見・移動・読取失敗は再選択へ止める。Rootのunsupported/permissionはserver投影をそのまま表示し、UUIDやACLから補完しない
- 現在名と変更先名を別に表示し、名前のNFC/空白処理、名前・理由の検査を既存folder規約から再利用する。編集中の基準名・revisionが送信前のfresh readと違えば、希望名と理由を保持して明示的な見直しへ止める。最新revisionへ黙って差し替えて外部更新を上書きしない。背景GETで入力を消さない
- 操作ID、pathの対象ID、期待revision、正規化済み変更先名、理由、読取根拠と送信時名を固定する。現在名と変更先名からexpectedChangedも固定する。no-opでも既存APIへ送って結果を確認し、GUIだけで成功にしない
- receiptは操作ID・対象ID・changed・resultingRevision・厳密な日時を照合する。実変更は期待revision+1、正規化同名のno-opは据置。実変更で+1を安全整数として扱えない場合は送信せず理由を示す。no-opは安全整数上限の据置まで扱い、create側の既存境界は狭めない
- 不正receipt・応答不明はUNKNOWNとして同じpath/body/contextだけを保持・明示再送する。現在readに依存させず、後続403/404等で初回未実行と断定しない。beforeunload警告とメモリー内だけの保持を使い、ブラウザー永続storageを追加しない
- 初回の確定拒否を見直すときだけ、同じ保存済み対象IDの実再選択による新しい読取根拠を候補にできる。fresh確認とgeneration/navigation/store identityが一致したときだけ新規入力へ戻る。確定拒否は利用者が明示確認して終了することもできるが、pending/unknownの破棄は一切許さない
- createとrenameのstoreは分けて保持し、どちらかがpending/unknownの間は他方の新規開始を止める。送信前read完了後にも両storeを実際に確認する。既存の固定要求の再送や確定結果の確認は継続でき、他方の要求・receipt・離脱警告を消さない
- read中の取消、選択変更、戻る/進む、別navigation、unmount後の遅延応答からPATCHを開始せず、別の要求をclearしない。POST/PATCH開始後の結果は元対象へ保持し、勝手にnavigateしない
- 成功後は元親Gと対象Pのfolder query prefix、documents/documentの名前projectionを無効化する。現在選択がPのときだけquery外のchosenFolder/folderContextコピーを破棄し、URLのP ID・文書filter/selection・別の選択Qは保持する。現在名はID表示とツリー再選択で確認し、過去replay receiptのrequest.nameを現在名へ直接注入しない
- 名前順で位置が変わるため、読取中・失敗・取得済み範囲外を明示する。再読取失敗で確定成功を取り消さず、存在しない/空一覧/完全snapshotと断定しない。Document/Version/親Folderのrevisionへrename receiptを代入しない

## 実装・検証

1. 薄いSDK adapter、改名専用store/dialog、既存Root createとの直接guardと一覧の配線を最小追加する。入口・PATCH契約・no-op/receipt・stale見直し・fixed replay・数値上限・相互race・名前cache/選択保持の純粋/DOM反例をREDから確認する。基点の全GUI597件を保持し、型/buildを検証する
2. 既存Organization Rootの2+2ケースを最小拡張し、既存の選択親配下にGUIで作成した子1件だけを改名する。Root snapshot・201件の名前/配列・Work本文は保持する。元createのrequest/receipt/childを変更せず、rename後の現在child期待を別に保持し、同ID・実PATCH/replay/現在権限・HTTPサーバー再起動後の表示を確認する。小さく成立する範囲でno-opも確認し、実施しなければDOM資格と区別する
3. 全GUI・型/build、runtime純粋・型・collection、独立source/spec/品質レビュー、日本語Draft、同一headの既存hosted CI・owned cleanup・公開artifact0を確認する。main mergeは親、実サーバー反映は所有者の手動操作

## 制約と資格

backend/API原本/生成SDK/lock/CI/Playwright構成は変更しない。移動・ACL・既読・Search/Audit/Toolboxの作業は含めない。ローカルlistener/socket/browser/DB/Cargo・画像保存を行わず、実通信は既存hostedの使い捨てPostgreSQL18.6・固定2名・Chromiumだけ。120秒/test・retries0・画像/trace/video off、既存有限診断とcleanupを保持する。golden/full visual・全headers喪失・対象PC導入/backup/restore/PostgreSQLプロセス再起動・本番認証の未資格は維持する。

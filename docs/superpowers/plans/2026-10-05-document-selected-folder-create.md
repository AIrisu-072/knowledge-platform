# 選択したフォルダー内への子フォルダー作成

## 目的と承認範囲

既存内部処理を通常GUIから使いたいという所有者の指示（`Sentinel_cef3cbfde698819198d9ed687212a610`）に従う、小さい追加UIである。PR78統合main `09f79a2635b09510e2d0bdeb530ba77881a70e37` を基点とし、既存Root作成の仕組みを、ツリーで選択した非root親にも接続する。新backend・API・権限・業務状態の追加ではない。改名は別の変更結果・no-op照合が必要なので、今回は扱わない。

## 固定する設計

- Root直下の既存入口に加え、ツリーで実際に選択した非root親への入口を設ける。選択行のID・その行を載せた親ID・取得済みページ数を読取の根拠として保持する。直URLだけで根拠が無い場合は、名前・revision・親関係を推測せずツリー再選択を案内する
- 新しい送信の前に、選択行を載せた親のchildrenを先頭から明示的に読み直す。以前取得済みだったページ数を上限とし、その読み直しで新しく返ったopaque nextCursorだけを順に使う。折り畳んだqueryも実際に読み直す。対象IDと親関係を確認できなければ送信せず、再選択へ止める。全ツリーの自動探索や全件保証を追加しない
- 見つかった現在行のrevisionと、対象自身のchildren readのcreateFolder capabilityを使用する。他の親のcapabilityを流用しない。最終backend再認可・OCCが正本であり、readから送信までの競合は既存409等で扱う
- Rootと選択親は1つのdialog/controllerと1つのQueryClient内操作storeを共有する。固定operationId・新folderId・parentFolderId・expectedParentRevision・name・reasonと、表示・再読取用の小さいUI contextを保持する。未解決要求がある間、別のRoot/親宛ての新規作成は開始しない
- 再表示や別navigationでは実際の保存済み作成先名・IDを表示し、現在の選択へ送信先を置き換えない。unknownは現在のreadに依存せず同じ固定要求だけを明示再送する。後続403/404を初回未実行の証明にしない。初回の確定拒否を見直す場合も、保存済み読取根拠を使うか元対象の再選択まで止め、別親のreloadへ接続しない
- 新しい送信前のread中にキャンセル・選択変更・navigationがあれば、遅延したreadからPOSTを開始しない。pending/unknown要求自体は消さず、beforeunload警告と管理者への確認案内を維持する。ブラウザー永続storageへpayloadを保存しない
- 成功receiptは従来どおりoperationId・新folderId・changed=true・子のresultingRevision=0を照合する。親revisionへ子のrevisionを代入しない。作成先の通常capability queryとページ列を既存prefixで無効化し、成功後の再読取失敗を作成失敗へ変換しない

## 実装・検証

1. 既存Root component・store・API・CSSを最小再利用し、選択先・fresh read・cache境界・非同期選択変更・再表示・固定再送・OCC・disabled理由のDOM/純粋反例をREDからGREENにする。Root既存回帰とページ送りを保持する
2. 既存の画像なしOrganization Root受入を最小拡張する。201件の末尾を通常GUIで選び、その配下へ子を作り、現在API/GUIと既存HTTPサーバー再起動後の同じ子を確認する。既存Root snapshot・201件・Work受入を保持し、新runner・新case群・外部通信・画像を追加しない
3. 全GUI・型/build、必要なruntime純粋・型・collection、独立source/品質レビュー、日本語Draft、同一headの既存hosted CI・owned cleanup・公開artifact0を確認する。main mergeは親、実サーバー反映は所有者の手動操作

## 制約と未取得の資格

backend/API原本/生成SDK/lock/CI構成は変更しない。改名・移動・ACL・既読・Search作業・新しい検証基盤は含めない。ローカルlistener/socket/browser/DB/Cargoを起動せず、実通信は既存hostedの使い捨てPostgreSQL18.6・固定2名・Chromiumだけとする。既存120秒・retries0・画像offを維持し、macOS golden/full visual・全headers喪失・対象PC導入/backup/restore/PostgreSQLプロセス再起動の未資格を引き継ぐ。

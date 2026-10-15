# 文書段階試験の時間予測

smallの登録件数に比例しない再起動まで全件比例で見積もると、固定費の揺らぎで次段階が開始不可になる。工程別時間を使用し、固定費は一回分、その他は保守的な件数比例で予測する。製品SLOや10万件の合格を意味しない。

`phaseTime` はmonotonicの相対startMs/endMsを最大64区間で記録する。先頭0からmetrics.totalElapsedMsまで隙間・重複なく覆い、文書件数を照合する。HTTP時間と並行probeの合計を引かず、実際に待ったcheckpoint・journal・観測処理を各区間に含める。

固定区間は `restart-process-replacement` のみ。完了した操作と背景観測の待機後、所有human/agentの停止と起動・ready待機を行う。PG自体は再起動せず、serveの構成・readyは固定schema/SELECT1、固定storageとworker preflightを検査し、Document全件を走査しない。identity前後のDB/storage計測、負例、全件一覧、保持確認、close、未分類時間は件数比例に残す。OS競合による遅延をゼロにせず固定費にも安全係数を掛ける。

前段件数n、次段件数N、固定費F、前段全経過Tについて時間予測は `ceil(安全係数 × [F + (T−F)×N/n])`。安全係数2を維持する。固定区間以外の時間を免除しない。phaseTimeのない旧reportは従来の全体比例予測へ戻すが、不正なphaseTimeがあるreportはNOT_ADMITTED。旧runや別sourceのreportを現在runへ転用しない。

時間予測とは独立に、実経過時間と各deadlineによる停止、総80時間とcleanup枠、RSS/空きmemory/diskの開始判定と継続監視を維持する。再試行して通る結果だけを選ばない。失敗・NOT_ADMITTED・未実施をそのまま保存する。

起動方法と4GiB指定は[メモリ予算手順](document-load-local-memory.md)に従う。cleanな資格済みmain・新runID・空の専用ext4 DBでfresh small→1000→10000→100000を実行する。起動前に所有lock/PID/tmux・現在資源・重複runを確認する。実対象PC・本番認証/TLS・実アカウント設定は別途承認が必要。

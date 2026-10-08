# Organization予約公開schedulerの限定接続

## 承認と目的

2026-10-08 13:49 UTC、依頼者は確認待ち4点を進めると回答した。そのうち予約公開の提示内容は「現在のOrganization用の模擬利用者6名に対応させ、公開時点の権限を再確認し、取消・権限失効・再起動も試験する。本番認証への接続は別途」である。本追補は既存実行経路への限定接続で、新たな認可方式ではない。

## 接続する契約

- `KP_RUNTIME_MODE=organization-synthetic` を明示したschedulerだけが、同providerの `sales-01`、`office-01`、`review-01`、`approver-01`、`multi-role-01`、`delegate-01` を解決する。
- 既存Organization HTTP adapterと同じ本人Principal subjectだけを返し、Group、Work役割、委任をDocument権限へ変換しない。5分の有効期限は解決ごとに更新する。
- `poc` は従来の2主体だけを解決する。provider混在、未知主体、executorの本人化は拒否する。実在利用者や外部directoryを追加しない。
- 保存した依頼者を再解決し、現時点のReadとPublishを既存Document transactionで再確認する。`service/scheduler` は既存の監査上のexecutorであり権限の付与元にしない。
- DB時刻、DSI sandbox、公開品質、manifest、OCC、Publish operation ID、取消と終端処理、監査を変更しない。規範は `spec/data/transaction-consistency-requirements-v0.md` のT3a。
- 6主体の対応はscheduler内の閉じたmappingとする。Organization serverへの依存は試験専用とし、実adapterとの一致を試験する。製品依存を増やさない。

## 検証と除外

既存PoCの実process受入を維持し、Organizationでも実PostgreSQL・製品DSI・別scheduler processを用いて期限前非公開、取消後非公開、失権・未知主体・読取専用主体の拒否、停止中非実行、再起動後実行、競合時1回だけの公開と監査帰属を確認する。mockだけで実受入を代替しない。

本番Identity、TLS、資格情報、実権限、deployment、全体のactive pointerと共通導入手順は変更しない。実サーバー導入の資格は本試験では付与しない。

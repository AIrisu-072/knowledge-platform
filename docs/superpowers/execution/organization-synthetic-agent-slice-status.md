# Organization Browser PoC — 合成Agentの状況

## 2026-10-04 14:31 UTC — 最小縦断実装とローカル確認完了、hosted未受入

- 凍結4API、durable受付/単発dispatch/取消/result、実Documentの両主体認可、immutable Finding、共通Agent UIと既存HumanDecision/提出への合流を実装した。実executorは合成、本文分析や実LLM/MCP通信は行わない
- Rust5packageの純粋試験73 PASS、既存PostgreSQL transaction試験1件はcompile済みで明示ignored。GUI153件/19 suites、純粋runner14件、application/runtime型、schema freshness、production build、OpenAPI lint、strict Clippy、fmt、差分確認PASS。既存Webpack advisory3件を維持
- 既存journey/persistenceの2件をcollection-onlyで確認。attempt1の検証を保持し、attempt2の営業合成候補＋人間判断の明示共有、事務の別候補＋判断のprivate分離、再起動後のresult/operation回復を追加した。ローカルDB/listener/browserは実行していない
- 独立レビューの指摘により、内部unknownでqueued/runningを残す経路を条件付きoutcome_unknownで閉じ、成功済みを保持した。Agent由来とHuman側source確認をまたぐ開示全体の鮮度を確認。decoderへ新codeを反映し、回復した非終端receiptだけで進行を断定しない表示へ修正した
- [判断記録](../../decisions/2026-10-04-organization-synthetic-agent-poc.md)は実装者選定とその影響を示す。個々の案の所有者事前承認と偽らない。旧migration0001–0003/checksum、依存lock、既存Document/MCP implementationとworkflowは不変

次のexact action: 最終exact treeの限定レビューを完了し、新Draftへ公開する。同じ承認済み一時DB/固定2Human/Chromium/画像無しでAgentの保存・取消/fence・人間判断/提出・private・両HTTP server再起動/復元・cleanupと全CIを確認する。**新Agentの実DB/browser結果はまだ未取得**。後続実受入結果は当該Draftのexact-head CIとPR本文を参照する。

---


2026-10-04 13:45 UTC。状態: **Frozen契約の最小実装開始、未受入**。

- 基点は[PR57](https://github.com/AIrisu-072/knowledge-platform/pull/57) `d383baccddd5081687b500f064f6fce195a24816`、tree `2d6fd53f6c7edf3e1be44de1a046ebfc79018d72`。新branch `feat/organization-agent-slice` で受入sourceを保持する
- 前headの[通常CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37205217912)、[DSI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37205217991)、[Sandbox](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37205217880)は成功。実DB/transaction、2名の根拠・候補・3種判断・選択提出・差戻後private分離、両HTTP server再起動後復元、cleanupも成功。GUI129、Rust856/7skip、既存mock browser6、artifact0件
- [短い実装計画](../plans/2026-10-04-organization-synthetic-agent-slice.md)に従い、既存根拠の選択から合成Agentのimmutable Findingを保存し、既存HumanDecision/提出へ接続する
- 実Document認可、合成executor、MCP wire未実行を区別する。Human2名・同じ一時DB/Chromium・画像無しの範囲。実LLM/外部サービス/credential送信・本番運用は行わない

現在: Work lifecycle/ledger/fence、固定provider認可/owned dispatch、共通Agent UI/既存journey延長を実装する。ローカルDB/socket/listener/browserは実行せず、pure tests/compileと既存hostedのみを使う。

未確認: Agent専用source、永続化、取消/late output、result再認可、2名browser/restart/cleanup、通常CI。新しい成功はまだ主張しない。

## 13:59 UTC — 実装中の境界確認

- 生成API契約と共通Agent UIの入力/合成表示/切替破棄の純粋試験を準備。Rustは旧snapshotで失われた既存lockのcacheを公式registryから復元し、限定compile/TDD中
- source認可は1 sourceずつ実施する。各参照確認の前後をWork lifecycleでfenceし、Document内部のRevision/Historyは既存current認可を使用。取消後の後続source開始を防ぎ、汎用callback/queueは追加しない
- 新Agentの実DB/browser受入は未実行。前PR57の成功と混同しない

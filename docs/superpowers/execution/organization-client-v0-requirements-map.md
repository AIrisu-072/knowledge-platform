# Organization Client v0：元の要求との対応表

依頼者の指示には**0〜51**の番号が付いています（番号付き52節）。設計前に、親担当から転送された原文3部をすべて読みました。この表は索引であり、元の権限や現在のrepositoryの事実を置き換えません。元指示は依頼者message `Sentinel_1df724f240248191864d194a0538819c`です。前提は[正確な受入記録](https://github.com/AIrisu-072/knowledge-platform/pull/43#issuecomment-5952167525)により満たされています。

P0は[再構成](organization-client-v0-phase0-reconstruction.md)です。
P1は[Product/UX](../specs/2026-10-02-organization-client-v0-product-ux-design.md)です。
P2は[凍結Domain/API/Auth記録](../specs/2026-10-02-organization-client-v0-domain-api-approval.md)です。
P3は[正確なsource/画面の適格性確認記録](../specs/2026-10-02-organization-client-v0-ui-approval.md)で、最終capture起動CIと実原画像20枚のレビューにより検証済みです。P4/P5/P6は後続の順序付き作業で、完了の主張ではありません。状態を記録するのは各Phase記録であり、この元要求の索引ではありません。

| 元の§ | 要求 | 担当設計 / 証拠 |
|---|---|---|
| 0 | 開始前のPR43 exact source受入、現在gate、旧PRをmergeしない | P0記録と現在head対応表 |
| 1 | Documentのfeature/core/API/runtime/worker/scheduler/MCP/harness再利用 | P0 component表、P1§11、P5統合回帰 |
| 2 | Organization Clientの業務操作面、Search名称は不変 | P1§1〜3 |
| 3 | タスク/文書/検索、初期画面はタスク、主moduleを増やさない | P1§3、P3 navigation source |
| 4 | 営業と事務の2archetypeのみ | P1§4、P3の2source設計 |
| 5 | 営業のcontext中心の継続性 | P1§4、P3営業journey |
| 6 | 事務のWorkType/queue、同じWorkItem権限 | P1§4、P3事務journey |
| 7 | Reviewは事務WorkViewProfile | P1§4〜5 |
| 8 | Returnはworkflow/rework状態、第3layoutを作らない | P1§7、P2 return model、P3 scenario |
| 9 | Evidence、Finding、HumanDecisionの区別 | P1§8、P2構造化record |
| 10 | 両Context SurfaceにEvidence | P1§5,8、P3 module layout |
| 11 | 両archetypeを横断するAgent | P1§9、P3 journey |
| 12 | 文脈付きAgent Chatと認可されたcontext | P1§9、P2 context API |
| 13 | Transcriptは業務SSOTにしない、実行結果は構造化 | P1§9、P2 execution/result contract |
| 14 | HumanとAgent以外のEvidence producer | P1§8、P2登録 |
| 15 | WorkContext×WorkItem共通model | P1§4、P2 domain |
| 16 | Role、assignment、delegation、時間境界、二重帰属 | P1§6、P2 organization/auth |
| 17 | Department既定/candidate poolはTask ACLではない | P1§6、P2 policy |
| 18 | Workflow/step/transition/責任区間/instance | P1§6〜7、P2 workflow定義 |
| 19 | Eligibility≠assignment、queue≠非公開内容の可視性 | P1§6、P2認可テスト |
| 20 | 完了作業→snapshot→next ready、grantのコピーではない | P1§7、P2 atomic handoff |
| 21 | 不変return/rework履歴、最小attempt方式の評価 | P1§7、P2明示的な代案/選択 |
| 22 | Backendのwork_item_private/handoff/context_shared | P1§6、P2 direct/list/contextの強制 |
| 23 | 現在有効な認可の積集合 | P1§6、P2 policy/provider境界 |
| 24 | Attentionは導出し、lifecycleを増殖させない | P1§7、P2 projection |
| 25 | Profile field、role上書き、両方にEvidence | P1§5、P2 profile schema、P3 source |
| 26 | Context Moduleと4表示状態 | P1§5、P3 module状態表 |
| 27 | 論理Workspace、作成flow1つ、管理root | P1§10、P2 runtime、P3 dialog |
| 28 | 管理/明示/policy由来binding、古いaccessの取消 | P1§10、P2 resource policy |
| 29 | Accessible≠enabled | P1§10、P2 field/actionの分離 |
| 30 | Principal/device-localとsharedの区別、promotion、opaque locator | P1§10、P2 handoff/resource contract |
| 31 | Search不変、discovery≠auth≠execution、WIP編集なし | P0、P1§11、P2将来統合 |
| 32 | Tauri v2優先、同じReact/browser、featureの書直しなし | P1§11、P4証拠 |
| 33 | Runtime contract、browser FSは将来/低優先 | P1§11、P2 runtime interface |
| 34 | 範囲限定local broker、任意path/shell/逸脱なし | P1§11、P2 broker、P4攻撃テスト |
| 35 | 主window1つ、pop-outなし | P1§11、P3 navigation、P4 runtime |
| 36 | v0 sidecarなし、将来境界のみ | P1§11、P4範囲 |
| 37 | 本番identity延期、合成multi-principal/role | P1§13、P2 fixture auth、P6テスト |
| 38 | Shell/Collection/Work/Context/Action Surface | P1§3、P3 source |
| 39 | 具体source設計2つを完成 | P2凍結後のP3 |
| 40 | 同じ2layout上の10scenario | P1§12、P3状態表 |
| 41 | 両方でFinding→Evidence→HumanDecision→action | P1§9、P3 scripted interaction、P6 |
| 42 | 厳格なPhase0→1→2→3→4→5→6 | P1§13、statusと個別review |
| 43 | 4labelのPR/component再利用表 | P0 |
| 44 | 必須API面の全体、共通業務logic | P2 endpoint/command表 |
| 45 | EvidenceRecord/Finding/HumanDecisionのAPI/model分離 | P1§8、P2 schema/認可 |
| 46 | 現行公式Tauriの適格性確認と実runtime case | P4、文書では達成を主張しない |
| 47 | 合成unit/actor/role/workflow/evidence/Agent journey | P1§12、P2 fixture catalog、P6 |
| 48 | D1/D2/T1/W1/W2/C1/C2/T2/A1/E1のDraft分割 | P2 plan/delivery graph、mergeなし |
| 49 | 列挙したSTOP条件、命名/refactor/testでSTOPしない | P1§13、Phase固有gate |
| 50 | 忠実な形式化は事前承認済み、正確な範囲/blobを記録 | 独立review後の個別承認記録 |
| 51 | 最初の9項目報告、P1→2→3を継続してからTauri | P0の親担当報告、現在status |

後続Phaseの行は完了の主張ではありません。Phase1はUXの意味を凍結します。Phase2で具体的domain/API/認可を定めてから、Phase3のinteraction sourceへ進む必要があります。

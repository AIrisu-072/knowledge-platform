# Organization Client v0：Phase0の現状再構成

Snapshot：2026-10-02 12:22 UTC。実装のSSOTはrepositoryと実際のGitHub観測です。この文書は読取専用の現状再構成で、新しい製品受入の主張ではありません。

## 受入済みの前提

- [PR43](https://github.com/AIrisu-072/knowledge-platform/pull/43) source H2：`6103e4d4e3bb0d45ba03e1d2935492de7f11394a`、tree `f2e13eee0d7e1bfa71952c1da52a72cecb65fc9e`
- [依頼者の最終受入](https://github.com/AIrisu-072/knowledge-platform/pull/43#issuecomment-5952167525)は、開示済みの画面制限を含めた2026-10-02 12:16 UTCの承認を記録します。これにより要求された前提checkpointを満たします。CIだけで満たしたのではありません
- [PR46報告R2](https://github.com/AIrisu-072/knowledge-platform/pull/46)：`88628331f32e141c71944e9f7f093deb348f51ac`、tree `92416d3dc7d7cc1c549d5ead5aa8c8b1ef70c43d`。これは証拠であり、製品基準の代替ではありません。順序付きparentは報告R1 `f49866f0fef88d2735db062b83c3ad686e9097b6`とH2です。H2との差分は正確にMarkdownの証拠/status/runbook5pathです
- Local `4b15a492`と`851f8aee`は、それぞれ同じtreeを持つsource/report準備commitであり、hosted commitではありません。共有object databaseで、完全なhosted H2/R2 objectと履歴を独立検証しました
- 新しい隔離branch `design/organization-client-v0`は実H2から開始します。既存Document/Search/Audit branch/worktreeは変更していません。既存PRのmerge/closeは行っていません

ソースにcommitされたActive/Status記録は、その後のhosted検査と依頼者受入より前のものです。保留の記述は過去の状態として保持します。リンク先のexact-head記録でこの時系列を明確にし、凍結前提を書き換えません。逆に、後の統合検証によって、古い異なるsource headが検証済みになることはありません。

## PR36〜43の現在head再利用表

記載したPRはすべてopen、Draft、未mergeです。下記workflow IDは各exact headについて取得したもので、別の検証済みbranchからコピーしていません。

| PR | 分類 | 正確なhead | 現在観測した検証gate | 再利用 / 制限 |
|---|---|---|---|---|
| [36](https://github.com/AIrisu-072/knowledge-platform/pull/36) GUI | 適応して再利用 | `706307b980beb540db0759ad772ed4debd42c289` | CI36946070147、DSI36946070021、Sandbox36946070032 SUCCESS | 凍結GUIの意味と機能基盤。自身の過去G9完了処理は保留。後のGUI修正を含む統合H2 sourceを使う |
| [37](https://github.com/AIrisu-072/knowledge-platform/pull/37) Runtime設計/計画 | そのまま再利用 | `dc9eb9bde55777934c100ce79c8c2b43ca8430eb` | CI36927672334 SUCCESS | Composition、固定identity、明示的migration/bootstrap、same-origin、別schedulerのcontract。設計CIはruntime受入ではない |
| [38](https://github.com/AIrisu-072/knowledge-platform/pull/38) MCP設計/計画 | そのまま再利用 | `5a2b114964ddbe7d38dd6a5fe9b70fdad2cb56f1` | CI36927833055 SUCCESS | 生成client、読取9tool、stdio、固定Agent endpoint、DB/Storageへ直接アクセスしない境界 |
| [39](https://github.com/AIrisu-072/knowledge-platform/pull/39) PoC受入計画 | そのまま再利用 | `7d49a7bbde4d26bd072732c41cebe85a419fd4dd` | CI36928002797 SUCCESS | 同run/exact-headのprovenance、実composition、failure/recovery、正確な証拠分類 |
| [40](https://github.com/AIrisu-072/knowledge-platform/pull/40) Search WIP | ブロック中 / 未解決 | `a945fbd32145a3109e35cb9cb056cea052698138` | CI36945866252とDSI36945866208 FAILURE、Sandbox36945866263 SUCCESS | 読取専用の将来統合contractのみ。code依存、修正、完了主張、改名なし |
| [41](https://github.com/AIrisu-072/knowledge-platform/pull/41) Runtime実装 | そのまま再利用 | `513529f5c256ee6e439dcdd16e746b417912ab2c` | CI36965695310、DSI36965695308、Sandbox36965695318 SUCCESS | 既存実server、明示的command、固定profile、seed、別scheduler、harness。統合H2がsource基準 |
| [42](https://github.com/AIrisu-072/knowledge-platform/pull/42) MCP実装 | そのまま再利用 | `143ce4d5a07abbdca076f1f90c8e979234814be1` | CI36972476928、DSI36972476913、Sandbox36972476940 SUCCESS | 既存の実stdio adapterとruntime受入。Agent書込権限や実LLM検証は推測しない |
| [43](https://github.com/AIrisu-072/knowledge-platform/pull/43) 統合受入 | 適応して再利用 | `6103e4d4e3bb0d45ba03e1d2935492de7f11394a` | 通常CI36987407999、DSI36987408029、Sandbox36987408087、capture CI36989549579 SUCCESS | 依頼者が受け入れたexact source。N2/V2と後の報告R2はrun対象が異なり、dataset/証拠を混ぜない |

C0を消去しません。PR36の変更していない自身のheadのG9記録は、独立した未完了成果物のままです。H2によって遡ってPASSにせず、依頼者の明示的H2受入後にOrganization開始の追加gateを作ることもしません。

## Componentの再利用と適格性確認

分類用語：**そのまま再利用（reuse unchanged）**はauthoritativeな動作を保持します。**適応して再利用（reuse with adaptation）**は外側のshell/runtime/test構成を変え、継承した業務上の意味は変えません。**置換済み（superseded）**は前提や過去状態に適用する語で、source削除の許可ではありません。**ブロック中 / 未解決（blocked / unresolved）**は実装の適格性を示しません。

| Component | 分類 | 現在の実装 / 証拠 | Organizationでの扱い |
|---|---|---|---|
| Document GUI | 適応して再利用 | `apps/document-web/src`、H2実browser journeyと開示済みV2画像レビュー | React feature、生成client/BinaryTransportBridge、Version/Revision/OCC、capability、Diff、原fileの意味を再利用。既存app shell/初期画面はDocument固有で、範囲限定composition変更が必要 |
| Document Domain/Application/API | そのまま再利用 | 既存crateとOpenAPI3.2.1、凍結承認済みの意味 | Documentのauthoritative境界を維持し、Task/Agent UIへ複製しない |
| document-server | そのまま再利用 | `crates/document-server/src/composition.rs`、実production adapterとroute composition | Document composition rootとして再利用し、Organization Domainにはしない。desktop sidecarや重複業務serviceなし |
| migrate/bootstrap | そのまま再利用 | `main.rs`、`bootstrap.rs`、schema互換性 | 明示的な使い捨てDB commandのみ。`serve`はmigration/seedしない。本番migrationは未承認 |
| Static identity | 適応して再利用 | `identity.rs`：固定`poc-human` / `poc-agent`、context期限更新 | 合成runtime接続境界のみ。header/query/body/cookieでidentityを選べない。既存の凍結動作を改名したり、本番identityと呼んだりしない |
| PostgreSQL/FileStorage | そのまま再利用 | Restart identityとrow/file assertionを持つ実authoritative共有状態 | Server所有のauthoritative providerを再利用。desktop pathは共有storageやhandoff参照にはしない |
| Scheduler | そのまま再利用 | 別process、requesterの現在権限、`service/scheduler`のaudit帰属 | 別process、元のrequester/executor区別、OCC、業務結果のexactly-onceを保持。帰属名で権限を与えない |
| 実runtime harness | 適応して再利用 | `tools/document-poc-runtime`、実worker、browser/API/MCP/DB/storage | 後続拡張は承認済み受入範囲内のみ。mockをruntime証拠と数えず、新artifactへcapture policyを自動継承しない |
| Document MCP | そのまま再利用 | `apps/document-mcp`、読取9tool、実stdio | 読取capabilityを再利用。Agent Chatは新しいinteraction構成であり、書込tool追加やAPI認可回避の許可ではない |
| Human/Agent受入 | そのまま再利用 | N2/V2全22stage、browser11/11、persistence1/1、Agent9group | 受入済みの前提証拠のみ。新Organization sourceは固有のexact-head gate/scenarioを満たす必要がある |
| Static serving | 適応して再利用 | `web.rs`、human限定build済みdist、予約API/health、範囲限定canonical path | Browser profileは対応を維持。Tauri asset/origin/transportは明示的検証が必要。CORS/CSP/path ruleを弱めない |
| Browser限定の前提 | 置換済み | `main.tsx`は`/`を`/documents`へredirect、shell linkは絶対path、clientはsame-origin、Webpack publicPathは`/`、native browser file input | 機能の意味を保ちruntime/navigation構成を分離。Browserにdesktop local-resource capabilityがあるとはしない |

明示的な分類：GUIは**適応して再利用**。document-server、migrate/bootstrap、PostgreSQL/FileStorage、schedulerは**そのまま再利用**。static identityは、別途範囲を限定したmulti-principal Organization合成fixtureのためだけに**適応して再利用**し、Documentの固定process identityを保持します。harnessは**適応して再利用**、Document MCPは**そのまま再利用**、Human/Agent受入は前提証拠として**そのまま再利用**し、新Organization証拠を別途必要とします。GUI static-serving前提はruntime contract経由で**適応して再利用**します。browser限定product/初期画面の前提は、依頼者承認済みの同React・desktop優先Organization shellで**置換済み**です。C0自身のheadの完了処理とSearch WIPは**ブロック中 / 未解決**です。これらの分類はcomponent削除を許可しません。

### 引き継ぐ証拠の制限

受入済み13画像には、01の選択detail読込中（02が別途ready contextを表示）、02の長い一覧titleの切れ、08のfull-page/fixed-viewport scrim動作、09の公開後選択でWORKINGのみを使うfallback、11/12の狭いtrace/reload間隔が含まれます。これらの制限込みで受け入れられ、欠点なし/全画面settledとは認定していません。Text comparatorの非対応multiline alignmentはUnknown/Noneのままで、「変更なし」ではありません。既存の操作を妨げないmotion、keyboard/focus、明示的timezone/offset、backend確認後の成功表示contractは必須のままです。

## 独立Auditの境界

[PR44](https://github.com/AIrisu-072/knowledge-platform/pull/44) head `82d2150b46e1ac680aa685b6e5e7b0e8b936ce4b`は別の設計作業です。[PR45](https://github.com/AIrisu-072/knowledge-platform/pull/45) head `f616b7207fc29cc721e7767d78d814307ac24be3`が検証するのはschema/contractのみです。元のnormal-ACL/reason保持の制限はdeferredのままです。A3 store/delivery/security適格性確認は、ここでの依存先でも完了capabilityでもありません。

将来のhandoffは、Organization Phase1/2後に、安定したactual-principal、acting-responsibility、resource、correlation識別子と、別途レビュー済みでversion付き・項目限定のmetadata catalogを受け取ります。Auditは耐久性のある説明責任の証拠であり、業務履歴の権限、KPI/経営分析、transcript保存、personal memoryではありません。現在の必須source-transaction staging ruleを維持し、generation/staging/delivery/store/verificationを区別します。非公開draftやEvidence本文をgeneric Auditへコピーしてはいけません。この設計branchではAudit codeの取込や修正は行いません。

## ローカル検証の境界

- Clean H2 worktree/tree同一性とsource/report履歴：PASS
- `git diff --check`：新文書追加前にPASS
- 基準の`node --test tools/api-contract/contract.test.mjs`：新worktreeで宣言済みRedocly依存が不足し、contract assertion前にBLOCKED。Host既定Node24.19.0は固定24.21.0と異なる。installや製品失敗は推測しない
- Rust/build/browser/依存install：NOT RUN。共有diskは約2.2GiBで、下限1.5GiBと重いbuildの占有直列化を保持
- 製品source、lock、workflow、static-serving、identity、database、既存受入artifactは変更していない

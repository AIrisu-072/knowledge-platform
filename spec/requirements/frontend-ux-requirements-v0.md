# Frontend / UX Requirements v0

- 状態: v0
- 対象: Knowledge / Document Platform のHuman UI全体
- 位置づけ: Operational Design System / Frontend Architectureの上位要求
- 正式サポート対象: Desktop業務端末
- 主入力: Keyboard + Mouse
- Tablet / Mobile: v0では正式サポート対象外
- 実装技術方針:
  - TypeScript + ReactをPreferred implementationとする
  - Motionは採用前提だが、Presentation補助として扱う
  - Router / State Library / Component Library / CSS Framework / Build Toolは後続選定
  - 本RequirementsはReactその他の具体技術に依存しない

---

## 1. Purpose

本Requirementsは画面レイアウトやComponentの見た目を先に固定するものではない。

目的は、**業務UIを一貫して導出するための共通契約**を定義することである。

```text
Business / Domain
      ↓
Feature UX Context
      ↓
Interaction / State Requirements
      ↓
Information Priority
      ↓
Presentation
      ↓
Component / Motion
```

最上位原則:

> **見た目の一貫性より、業務状態の認識・判断・処理速度の一貫性を優先する。**

UIはBackendの状態を分かりやすく提示する層であり、業務状態の正本ではない。

---

## 2. Scope

### 2.1 本Requirementsで固定するもの

- Feature UX Context
- UI State Semantics
- Backend authoritative principle
- Optimistic Update policy
- Risk-based Interaction
- Keyboard Interaction
- Motion Contract
- Error / Partial / Conflict presentation
- Perceptual Rules
- Cognitive Load Allocation
- Accessibility baseline
- UX Performance Budget
- Large-data / High-frequency UI requirements
- Continuous UX Improvement
- Frontend Architecture Boundary
- State Separation
- API / Validation Boundary
- UX Testing Contract
- Information Architecture
- Data Freshness / Staleness
- Trust / Provenance
- Bulk Operations
- Selection Stability
- Navigation Context Preservation
- Content / Data Presentation Semantics
- Status Vocabulary
- Validation Presentation
- Design Token方針
- Technology Independence

### 2.2 本Requirementsでは固定しないもの

- Button library
- Table library
- Dialog implementation
- Router
- Server State library
- Client State library
- CSS framework
- Build tool
- Motion libraryの具体API利用方法
- Design Tokenの具体値
- Component寸法
- 画面ごとの密度
- Page layout
- 配色パレット
- Font family

これらは本Requirementsを満たすことを条件として後続で選定する。

---

## 3. Feature UX Context

各Feature/UIは実装前に、最低限以下の短い仕様を持たなければならない。

```text
Feature UX Context

Task Context
Frequency
Required Information
Decision
Error Consequence
Primary Interaction
```

### 3.1 Task Context

このUIで利用者が完了させる業務を記述する。

### 3.2 Frequency

以下を記述する。

- 利用頻度
- 連続処理か単発処理か
- 1回の業務で扱う件数の想定

### 3.3 Required Information

判断・操作時に同時に認識する必要がある情報を記述する。

### 3.4 Decision

利用者が何を判断する必要があるかを記述する。

### 3.5 Error Consequence

以下の影響を記述する。

- 見間違い
- 誤操作
- 操作漏れ
- 古い情報の利用
- 誤った対象への操作

### 3.6 Primary Interaction

主操作を記述する。

例:

```text
keyboard
mouse
bulk operation
sequential processing
search-driven operation
comparison
review / approval
```

Feature UX ContextがないFeatureはUI実装へ進めない。

---

## 4. UIを業務文脈から導出する

画面密度・レイアウト・Component選択を先に決めない。

```text
Task Context
    ↓
Required Information
    ↓
Decision
    ↓
Information Priority
    ↓
Interaction Sequence
    ↓
Layout / Density
```

したがって「金融業務だから常に高密度」とはしない。

- 同時視認すべき情報が多いFeatureは高密度になり得る
- 重要判断へ注意を集中させるFeatureは中密度・低密度になり得る

密度は目的ではなく結果である。

---

## 5. State Semantics

UI Interaction Stateの共通語彙:

```text
Idle
Pending
Success
Warning
Error
Conflict
Partial
Disabled
```

これらはDomain Stateとは分離する。

例:

```text
DocumentVersion = PUBLISHED
```

はDomain Stateであり、

```text
Pending
Conflict
Success
```

はUI Interaction Stateである。

両者を混同しない。

---

## 6. State Presentation

状態は色だけで表現しない。

最低限、必要に応じて以下を組み合わせる。

```text
Text / Label
Icon / Shape
Position / Grouping
Color
```

状態表示はToastだけに依存しない。

利用者が操作した対象自体へ状態を反映することを優先する。

例:

```text
対象行
├─ Pending
├─ Success
└─ Error
```

---

## 7. Backend Authoritative

業務状態の正本はBackendとする。

```text
Rust Backend
    ↓
Server State
    ↓
Application / View Model
    ↓
Presentation
```

Frontend local stateは主として以下に限定する。

- 入力途中
- selection
- panel open / close
- focus
- animation
- temporary optimistic state

Server Stateを別Global Storeへコピーし、二重の正本を作らないことを原則とする。

---

## 8. Optimistic Update Policy

Optimistic Updateを一律適用しない。

### 8.1 可逆・低リスク操作

例:

- ReadState
- 軽微な表示設定
- reversible preference

許可:

```text
即時反映
↓
Backend request
↓
失敗ならrollback
```

### 8.2 高影響・重要操作

例:

- Document publish
- AccessPolicy change
- 重要データ更新

必須:

```text
即Pending
↓
Backend commit
↓
成功後に確定表示
```

成功前に成功済みと表示しない。

方式はFeature UX Contextの`Error Consequence`から決定する。

---

## 9. Risk-based Interaction

確認Dialogの数ではなく、誤操作リスクを下げる。

```text
低リスク・可逆
→ 即時実行 + Undo

中リスク
→ lightweight confirmation または Undo

高リスク・不可逆
→ 明示的確認

特権操作
→ 明示的確認
→ server-side authorization
→ Audit Event
```

「安全のためにすべて確認Dialog」は禁止する。

確認の常態化による認知的な形骸化を避ける。

---

## 10. Keyboard Interaction

Keyboard対応はAccessibilityだけでなく、業務効率要件として扱う。

高頻度Featureでは以下のkeyboard pathを検討する。

- 主要操作
- 主要移動
- 選択
- 確定
- キャンセル
- 検索
- 次対象への移動

Shortcut keyそのものはFeature単位で決める。

Design Systemで共通化するもの:

- focus management
- shortcut conflict回避
- discoverability
- disabled状態
- input fieldとの衝突回避
- OS / Browser標準shortcutを壊さない

v0では利用者によるキー割当カスタマイズを要求しない。

---

## 11. Motion Contract

Motionは正式採用対象とする。

役割:

> **状態変化・空間関係・注意の移動を認識しやすくするPresentation mechanism**

許可される代表用途:

- 行追加・削除時の位置関係維持
- panel開閉の空間関係提示
- 状態変更の短いfeedback
- focus / attention guidance

禁止:

- 業務処理を待たせる
- 状態確定をanimation完了まで遅らせる
- animation完了まで次操作をblockする
- 装飾目的だけで高頻度に動かす
- animation completionをbusiness state判定に使う

原則:

```text
T_motion completion
≠
operation completion
```

---

## 12. Error / Partial / Conflict

`Error Handling & Resilience Requirements v0` に従う。

```text
RFC 9457
    ↓
stable error code
    ↓
Application / View Model
    ↓
UI representation
```

UIは人間向けmessage文字列を機械判定に使わない。

最低限以下を別状態として扱う。

```text
Error
Conflict
Partial
```

例:

```text
検索結果あり
+
一部Source取得失敗
```

は全面Error画面にしない。

---

## 13. Perceptual Rules

利用者が長文を読まなくても、状態・優先度・関連性を迅速に認識できることを目指す。

優先順:

```text
Position
↓
Grouping / Proximity
↓
Typography / Size / Weight
↓
Icon / Shape
↓
Color
↓
Motion
```

Color / Motionを情報の唯一の媒体にしない。

---

## 14. Information Priority

Feature UX Contextの`Required Information`と`Decision`から各情報を分類する。

```text
Primary
= 判断・処理に必須

Secondary
= 判断補助

Supporting
= 必要時に参照

Hidden-on-demand
= 通常業務では不要
```

「保存されているから表示する」ではなく、「判断に必要だから表示する」を原則とする。

---

## 15. Cognitive Load Allocation

目的は認知負荷をゼロにすることではない。

> **利用者が本当に判断すべき箇所に認知負荷を集中させる。**

原則:

- 重要な判断材料を離しすぎない
- 比較対象は同時視認可能な配置を優先
- 内部ID等、通常判断に不要な情報は主画面から外す
- 同じ意味を異なる表現で乱立させない
- 重要操作直前に必要情報を再提示できる
- 状態変更後に「何が変わったか」を追跡できる

---

## 16. Accessibility

最低基準:

```text
WCAG 2.2 AA
```

必須:

- Keyboardのみで主要業務を完結可能
- Focus位置が常に視認可能
- 状態を色だけに依存させない
- 十分なcontrastを確保
- Pointer-only操作を作らない
- Error位置・原因を識別可能
- Reduced Motionを尊重
- Motion無効化でも意味・状態が失われない
- Focus orderが業務動線と一致
- Semantic HTMLを優先
- ARIAはnative semanticsで不足する場合に利用

Accessibilityは別モードではなく標準UIに組み込む。

---

## 17. UX Performance Budget

全Featureに共通最低基準を置き、必要に応じてFeature固有SLOを追加する。

初期Budget:

| 対象 | Budget |
|---|---:|
| 入力 → 視覚feedback開始 | **≤ 50 ms** |
| local state変更 → 操作可能 | **≤ 100 ms** |
| keyboard操作応答 | **≤ 100 ms** |
| Motionによる次操作blocking | **0 ms** |
| interactive animation | **60 fps目標** |
| main-thread long task | **原則50 ms未満** |

Backend latencyとは分離して測定する。

```text
T_input
= user action → feedback開始

T_usable
= user action → 次の業務操作が可能

T_backend
= request → authoritative response

T_motion
= animation完了
```

必須:

```text
T_motion が T_usable を遅らせない
```

---

## 18. Loading / Pending Feedback

非常に短い処理でspinner等がちらつくことを避ける。

短時間で完了する読み込みでは、必要に応じてloading表示を遅延できる。

ただしserver confirmationが必要な重要操作は即座に`Pending`を示す。

Loading / Pending / Success / Errorを同一表現で曖昧にしない。

---

## 19. Large-data / High-frequency UI

大量データ前提のFeatureでは必要に応じて以下を利用する。

- virtualization
- windowing
- pagination
- incremental rendering
- stable selection
- keyboard navigation
- render scope minimization

全一覧へvirtualizationを強制しない。

Featureのデータ量・Frequency・比較要件から導出する。

---

## 20. Continuous UX Improvement

初期UIを完成版として扱わない。

Lifecycle:

```text
Feature UX Context
       ↓
Initial UI
       ↓
Internal operation
       ↓
Qualitative feedback
       +
UX telemetry
       ↓
Improvement
       ↓
Feature UX Context更新
```

### 20.1 定性的Feedback

収集候補:

- 見つけにくい情報
- 見間違い
- 余計な操作
- keyboardで詰まる箇所
- 状態が分かりにくい箇所
- 待っていると感じる箇所
- 比較しづらい情報
- 誤操作しやすい導線

### 20.2 UX Telemetry

候補:

- task duration
- interaction count
- navigation count
- rollback / error rate
- search reformulation
- keyboard vs pointer path

個人の業務評価・監視を目的としない。

Feature単位・操作単位の集約分析を優先する。

---

## 21. Operational Design System

v0では具体Component Libraryを定義しない。

固定するのは以下である。

```text
Semantic State
Interaction Rules
Perceptual Rules
Accessibility
Performance
UX lifecycle
```

後続のComponentはこの契約へ適合しなければならない。

---

## 22. Frontend Architecture Boundary

最低限、以下の責務を分離する。

```text
Rust Backend
    │
    │ OpenAPI Contract
    ▼
Generated API Client / Types
    │
    ▼
Server State
    │
    ▼
Application / View Model
    │
    ▼
Presentation
    │
    ├─ React Components
    └─ Motion
```

Presentation Componentから直接以下を扱わないことを原則とする。

- raw `fetch`
- API URL
- Backend固有transport detail
- business invariant

Preferred:

```text
API Client
   ↓
Application Action / Query
   ↓
View Model
   ↓
Presentation
```

---

## 23. State Separation

最低でも3種類を分ける。

### 23.1 Server State

例:

- Documents
- Search Results
- ReadState
- Permissions

### 23.2 Application / Workflow State

例:

- current operation
- selection
- pending command
- workflow progress

### 23.3 Pure Presentation State

例:

- panel open
- focus
- animation
- temporary visual state

Server StateをFrontend global storeへ複製して二重管理しない。

---

## 24. Technology Policy

現時点のPreferred implementation:

```text
Preferred Language = TypeScript
Preferred Renderer = React
Motion             = 採用前提
```

ただしArchitecture上の絶対条件にはしない。

固定するのは:

- Backendとの契約境界
- State分離
- View分離
- Accessibility
- Performance
- UX semantics

Router / State Library / Component Primitive / Build Toolは後続選定とする。

---

## 25. API / Validation Boundary

以下をFrontendでも利用する。

```text
OpenAPI 3.2.1
+
JSON Schema 2020-12
       ↓
Codegen
       ↓
TypeScript types
API client
Structural validation
```

Backend DTOやStructural Validation ruleをFrontendで手書き複製しない。

Frontend固有Validationは入力補助・早期feedbackとして存在してよいが、Backend validationを代替しない。

---

## 26. UX Testing Contract

Feature完成条件をFunctional correctnessだけにしない。

最低限以下を検証対象とする。

### 26.1 Semantic State

```text
Pending
Success
Warning
Error
Conflict
Partial
Disabled
```

### 26.2 Keyboard

主要業務経路がkeyboardだけで完結すること。

### 26.3 Focus

Dialog / Panel / Error / Navigation後のfocus位置が意図通りであること。

### 26.4 Accessibility

WCAG 2.2 AAに対する自動検査 + 必要な手動検査。

### 26.5 Performance

```text
T_input
T_usable
T_motion
```

がPerformance Budgetを満たすこと。

### 26.6 Motion

- 次操作をblockしない
- Reduced Motionでも意味が失われない
- animation completionを業務状態判定に利用しない

### 26.7 Error Handling

RFC 9457 stable error codeから正しいUI状態へmappingされること。

---

## 27. Visual Regression

主要FeatureについてVisual Regression Testを導入可能な構造を要求する。

目的:

- 列ずれ
- 状態badge切れ
- 重要情報の消失
- Focus Ring消失
- 意図しないlayout崩れ

等、業務上重要な視覚情報の破壊を検知すること。

全pixel完全一致自体を目的としない。

---

## 28. Feature Acceptance

各Featureは以下を満たして完成とする。

```text
Feature UX Context
+
Functional Acceptance
+
Accessibility Acceptance
+
Performance Acceptance
+
Error-state Acceptance
```

Feature UX ContextはAcceptance Testの入力として扱う。

---

## 29. Data Freshness / Staleness

必要なFeatureでは以下を明示可能にする。

```text
Last updated
Refreshing
Stale
Conflict
Partial
```

全画面に更新時刻を常時表示する必要はない。

Feature UX Contextの`Error Consequence`から必要性を判断する。

### 29.1 Silent Refresh

Background refreshは許可する。

ただし、利用者が読んでいる内容や選択対象を突然入れ替えないことを原則とする。

重要な更新がある場合は、利用者が認識可能な方法でreconcileする。

---

## 30. Trust / Provenance

情報の出所が判断材料になるFeatureでは以下を表示可能にする。

- Source
- Version
- Updated At
- Status
- Authority / Origin

特にSearchでは、必要に応じて以下を近接表示する。

```text
該当箇所
文書名
Source
Version
更新情報
```

LLM生成内容とSourceから取得した事実をUI上で混同しない。

---

## 31. Bulk Operations

高頻度・大量処理Featureでは以下を検討対象にする。

- multi-select
- bulk action
- select all / filtered selection
- continuous keyboard processing
- next item

Bulk操作では対象範囲を明示する。

```text
Bulk action
    ↓
対象件数 / 対象範囲を明示
    ↓
Risk-based confirmation
    ↓
Server processing
    ↓
成功件数 / 失敗件数 / Partial結果
```

Partial Successを単純な全面Errorにしない。

---

## 32. Selection Stability

selectionは表示位置ではなくstable identifierへ紐付ける。

```text
NG
selectedRowIndex = 14

Preferred
selectedResourceId = ...
```

sort / filter / refresh / paginationで別対象へselectionを黙って移さない。

対象消失・権限変更時も明示する。

---

## 33. Progressive Disclosure

通常判断に必要な情報を常時表示し、それ以外は必要に応じて展開する。

```text
通常判断に必要
→ 常時表示

必要になることがある
→ 近くから展開可能

技術・監査情報
→ 詳細 / secondary panel
```

重要情報をhoverだけに隠さない。

---

## 34. Navigation / Context Preservation

必要なFeatureでは以下を適切に保持する。

- search conditions
- sort
- filter
- selection
- scroll position
- opened result

例:

```text
検索結果
  ↓
詳細
  ↓
戻る
```

で、検索条件・位置を不必要に失わない。

復元時にはBackend stateとの整合を確認する。

---

## 35. Complete UI State Set

Happy PathだけをDesign対象にしない。

必要に応じて以下を設計する。

```text
Initial
Empty
Loading / Pending
Ready
Partial
Stale
Conflict
Error
Unauthorized
Unavailable
```

特に:

```text
0件
```

と

```text
検索失敗により結果が取得できない
```

を混同しない。

---

## 36. Content / Data Presentation Semantics

重要な値は、値だけでなく意味が分かる形で表示する。

必要に応じて:

```text
Value
Unit
Status
Effective time
Source / Version
```

を組み合わせる。

---

## 37. Date / Time Presentation

曖昧な日付表現を避ける。

例:

```text
避ける:
09/10/26

推奨:
2026/09/10
2026/09/10 14:32
```

内部machine-readable表現と利用者向け表示を分離する。

Timezone / 基準日時の意味が業務上重要な場合は明示する。

---

## 38. Number / Amount Presentation

必要に応じて以下を明示する。

- 桁区切り
- 単位
- 通貨
- 小数精度
- 正負
- 比率

表示フォーマットとBackend canonical valueを分離する。

---

## 39. Status / Label Vocabulary

同じ意味を画面ごとに異なる用語で乱立させない。

Semantic Stateに対応する標準Vocabularyを後続Design Systemで管理可能にする。

Domain固有語はDomain Languageを優先する。

---

## 40. Disabled / Read-only / Unauthorized / Unavailable

以下を同じ意味として扱わない。

```text
Disabled
= 現在の状態では操作できない

Read-only
= 閲覧可能だが変更対象ではない

Unauthorized
= 権限がない

Unavailable
= 一時的に利用不能
```

理由が必要な場合は利用者が理解可能な形で提示する。

---

## 41. Validation Presentation

OpenAPI / JSON Schema / RFC 9457のfield errorを可能な限り該当fieldへ紐付ける。

```text
RFC 9457
    ↓
JSON Pointer
    ↓
該当field
    ↓
inline error
```

必要に応じて画面上部にsummaryを追加する。

目的:

```text
何が不正か
+
どこを直すか
```

が同時に分かること。

---

## 42. Design Tokens

v0では具体値を固定しない。

後続Design Systemでは以下をToken化可能にする。

- Color
- Typography
- Spacing
- Radius
- Elevation
- Motion duration / easing
- Focus
- Semantic status
- Density

Tokenの目的はbrandingよりも、状態・認知・操作規則をFeature間で一貫させることを優先する。

Motion duration等をComponentへ無秩序に直書きしない。

---

## 43. Technology Independence

本RequirementsはTypeScript / React / Motionそのものを規範にはしない。

本当の契約は:

```text
Feature UX Context
State Semantics
Backend Authoritative
Accessibility
Performance Budget
Risk-based Interaction
Perceptual Rules
UX Feedback Loop
Architecture Boundary
```

である。

ライブラリ選定では「本Requirementsを自然に満たせるか」を評価する。

---

## 44. Library Selection Implications

本Requirements確定後に、少なくとも以下を比較する。

- React周辺構成
- Router
- Server State library
- Client / Workflow State library
- Motion
- UI primitives / Component library
- Form / Validation integration
- Accessibility tooling
- Visual Regression tooling
- Performance measurement tooling

選定時は成熟度・ライセンスだけでなく、以下を満たすかを評価する。

```text
state separation
headless / presentation separation
keyboard control
focus management
accessibility
non-blocking motion
performance
codegen integration
testability
```

---

## 45. Acceptance Criteria

本Requirementsに適合するFrontendは最低限以下を満たす。

- [ ] Desktop業務端末を正式サポート対象とする
- [ ] Feature UX Contextを実装前に作成する
- [ ] UI密度を業務文脈から導出する
- [ ] Backendを業務状態の正本とする
- [ ] Server StateとClient / Presentation Stateを分離する
- [ ] Risk-based Interactionを利用する
- [ ] 高頻度業務に効率的なKeyboard pathを設計可能にする
- [ ] Motionが次操作をblockしない
- [ ] WCAG 2.2 AAを最低基準とする
- [ ] 色だけで状態を伝えない
- [ ] UX Performance BudgetをFeature完成条件に含める
- [ ] Partial / Conflict / Staleを正式なUI状態として扱える
- [ ] 大量処理でstable selectionを維持できる
- [ ] Navigation後も必要な業務文脈を保持できる
- [ ] OpenAPI 3.2.1 / JSON Schema 2020-12とのCodegen境界を持つ
- [ ] RFC 9457 stable error codeをUIへmappingできる
- [ ] Visual Regressionを導入可能な構造を持つ
- [ ] Continuous UX Improvementを正式Lifecycleとする
- [ ] UX Telemetryを個人評価目的にしない
- [ ] TypeScript + ReactをPreferredとするがRequirements自体は技術非依存とする

---

## 46. Non-goals v0

v0では以下を決定しない。

- Component Library
- CSS framework
- Router
- State Management library
- Build tool
- exact Motion API / animation token values
- color palette
- exact spacing scale
- page templates
- final visual branding
- mobile / tablet support
- user-configurable keyboard shortcut mapping
- user-configurable density mode

これらは後続のLibrary Selection / Design System / Feature Designで決定する。

---

## 47. Related Specifications

- Architecture Contract v0
- Error Handling & Resilience Requirements v0
- Observability & Audit Requirements v0
- Transaction & Consistency Requirements v0
- OpenAPI 3.2.1 API Contract（後続）
- Development / Container / CI Architecture v0（後続）

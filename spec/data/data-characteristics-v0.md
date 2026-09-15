# Data Characteristics v0

## 1. 目的

本ドキュメントは、Knowledge / Document Architecture v0 および Logical Data Model v0 を前提に、各データの**性質・量・保持期間・更新特性・整合性要件・再生成可能性**を整理する。

目的は、PostgreSQL / SQLite 等のDB製品を先に決めることではなく、データ特性から以下を導出できる状態にすることである。

- 正本データと派生データの分離
- DB / File Storage / Search Index / Audit Store の責務分担
- 物理データモデル作成時の前提
- DB・ストレージ・検索基盤の選定条件
- 将来のバックアップ、冗長化、性能改善に耐えられる構造

本書は **v0** であり、未確認値は無理に確定せず、実測・ヒアリングにより更新する。

---

## 2. 現時点で確認・仮定している規模

| 項目 | v0の前提 | 確度 |
|---|---|---|
| 利用対象人数 | 約1,000人 | 高 |
| 現在の文書数 | 不明。100万件は超えない想定 | 低〜中 |
| データ蓄積期間 | 約12年 | 高 |
| 文書保存期間 | 原則永久保存 | 高 |
| 文書増加 | 継続的に増加 | 高 |
| 平均ファイルサイズ | 概ね1〜10MB程度。ばらつき大 | 低〜中 |
| 主な形式 | Microsoft Office系、PDF、ZIP、Windows業務ファイル | 中 |
| 1文書あたり版数 | 20〜30版は相当稀。平均値は未確認 | 中 |
| 1人あたり文書閲覧量 | 最大30件/日程度を設計上の仮値として利用可能 | 仮定 |
| 文書登録・改訂量 | 未確認 | 未確認 |
| 検索回数 | PoC後に人間・LLMそれぞれ実測 | 未確認 |
| LLM検索回数 | 設計上の回数上限は設けない | 固定方針 |
| 可用性SLA | 未確定 | 未確認 |
| RPO / RTO | 未確定 | 未確認 |

### 2.1 規模に関する基本方針

- 現時点では単一点の容量予測より、**増加し続ける前提**を重視する。
- 文書件数・版数・ファイル容量は、実測値が得られ次第レンジを更新する。
- 検索負荷は、LLMの探索回数を人工的に制限せず、PoC後の実測分布で評価する。
- ダウンタイム・バックアップ要件は後続要件で決定するが、将来それらを実現できない構造にはしない。

---

## 3. データ分類

全データを以下の4系統に分類する。

1. **原本データ**
   - DOCX / XLSX / PPTX / PDF / ZIP 等
2. **Document Platform の業務データ**
   - Document / DocumentVersion / Folder / Metadata / ReadState 等
3. **Search Platform の派生データ**
   - KnowledgeUnit / Lexical / Vector / Temporal / Structured 等
4. **運用・監査データ**
   - AuditEvent / Trace / Log / Metrics

重要な原則は以下である。

> Search Platform のデータは原則として派生データであり、Document Platform や外部 Knowledge Source を正本として再構築可能にする。

---

# 4. Document Platform

## 4.1 Document

| 特性 | 内容 |
|---|---|
| 役割 | 論理的な文書そのものを表す |
| 正本性 | **正本** |
| 更新特性 | タイトル・分類等は更新され得る。版そのものは DocumentVersion に分離 |
| 保持 | 原則永久 |
| 想定件数 | 最大100万未満を暫定レンジとする |
| サイズ | 小。主にID・状態・参照情報 |
| Transaction | 必要 |
| 再生成 | 不可。業務データとして保全が必要 |

### 設計上の注意

- Document と実ファイルを同一視しない。
- Document は複数の DocumentVersion を持つ。
- 現行版を示す状態は Document / DocumentVersion の整合性を保つ必要がある。

---

## 4.2 DocumentVersion

| 特性 | 内容 |
|---|---|
| 役割 | 文書の特定版を表す |
| 正本性 | **正本** |
| 更新特性 | 公開後は原則 immutable に近く扱う |
| 保持 | 原則永久 |
| 想定件数 | Document件数 × 平均版数 |
| サイズ | 小〜中。版固有metadataを含む |
| Transaction | **強く必要** |
| 再生成 | 不可 |

### 固定要件

- 既読状態は **`user × document_version`** で管理する。
- 新版公開時、旧版の既読状態をリセットしない。
- 新版には ReadState が存在しないため、自然に未読として扱われる。

---

## 4.3 FileObject / VersionFile

| 特性 | 内容 |
|---|---|
| 役割 | DocumentVersion に紐づく実ファイル |
| 正本性 | **正本** |
| 更新特性 | 版確定後は原則 immutable |
| 保持 | 原則永久 |
| 想定件数 | DocumentVersion と同程度、または複数添付がある場合それ以上 |
| サイズ | 概ね1〜10MB程度を中心に大きくばらつく |
| Transaction | DB上の参照整合性は必要。バイナリ自体はFile Storage側 |
| 再生成 | 不可 |

### 容量感の参考

原本1版のみの場合の単純計算:

| 文書数 | 平均1MB | 平均5MB | 平均10MB |
|---:|---:|---:|---:|
| 10万 | 約100GB | 約500GB | 約1TB |
| 50万 | 約500GB | 約2.5TB | 約5TB |
| 100万 | 約1TB | 約5TB | 約10TB |

版管理を含む場合はこれより増加する。

### 方針

- 実ファイルをRDBのBLOBへ集約することは現時点では前提にしない。
- DBにはFileObjectの識別子・パス・hash・MIME・サイズ等を保持し、バイナリはFile Storageへ分離する構成を基本候補とする。

---

## 4.4 Folder / Category

| 特性 | 内容 |
|---|---|
| 役割 | 人間向けの文書分類・ナビゲーション |
| 正本性 | 正本 |
| 更新特性 | 追加・名称変更・移動あり |
| 保持 | 長期 |
| 想定件数 | 文書件数より十分少ない |
| Transaction | 必要 |
| 再生成 | 原則不可 |

### 要求される特性

- 階層構造
- 移動時の整合性
- 循環参照防止
- 将来的な分類方式変更への耐性

---

## 4.5 Metadata

| 特性 | 内容 |
|---|---|
| 役割 | 文書種別、部署、公開日、適用日等の検索・管理用情報 |
| 正本性 | Document Platform内で管理する項目は正本 |
| 更新特性 | 項目により可変 |
| 保持 | 原則永久 |
| 想定件数 | Document / DocumentVersion に比例 |
| Transaction | 必要 |
| 再生成 | 項目による |

### 方針

共通項目を固定しつつ、Source固有・将来拡張項目を格納できる余地を持たせる。

固定候補:

- title
- document_type
- department
- created_at
- published_at
- effective_from
- effective_to
- revision_reason

拡張項目は柔軟なmetadata領域として保持可能にする。

---

## 4.6 ReadState

| 特性 | 内容 |
|---|---|
| 役割 | ユーザーごとの文書版の既読状態 |
| 正本性 | **業務状態として正本** |
| 更新特性 | 文書版を閲覧した際に作成・更新 |
| 保持 | 原則長期。保持ポリシーは後続検討 |
| 論理キー | **principal_id × document_version_id** |
| Transaction | 必要 |
| 再生成 | 原則不可 |

### 絶対条件

```text
ReadState = user × document_version
```

`user × document` では持たない。

### 想定負荷

利用者1,000人 × 最大30閲覧/日という仮値では、最大約30,000件/日のReadState書込イベントが想定される。

これは平均QPSとしては大きくないが、始業直後・通知直後等のピーク集中を別途PoCで計測する。

### Identity

- 新規ユーザー認証基盤は原則作らない。
- Windows統合認証から取得できる安定したPrincipal IDを利用する。
- 可能であれば表示名ではなくSID等の不変IDを利用する。

---

## 4.7 AccessPolicy

| 特性 | 内容 |
|---|---|
| 役割 | 文書・版へのアクセス可否を表現 |
| 正本性 | 正本 |
| 更新特性 | 組織変更・役割変更等で変化 |
| 保持 | 現行＋必要に応じ履歴 |
| Transaction | 必要 |
| 再生成 | 外部Identity/権限基盤との連携方式による |

### v0方針

認証・権限制御の詳細は後続設計とするが、Document API / Search API が同じIdentityを利用できる構造にする。

---

# 5. Search Platform

Search Platformは原則として**派生データ**を保持する。

重要原則:

> Search Index が全損しても、Knowledge Source から再構築可能であること。

---

## 5.1 KnowledgeSource

| 特性 | 内容 |
|---|---|
| 役割 | Document Platform、e-Gov、内部DB/API等の情報源を識別 |
| 正本性 | Search Platform上では設定情報 |
| 更新特性 | Source追加・設定変更あり |
| 保持 | 長期 |
| Transaction | 低〜中 |

初期候補:

- Document Platform
- e-Gov法令等の外部Knowledge Source
- 内部DB / API
- 将来の追加Source

---

## 5.2 CanonicalKnowledgeResourceSnapshot

| 特性 | 内容 |
|---|---|
| 役割 | Source Adapterから取得した情報を共通形式へ正規化したスナップショット |
| 正本性 | **派生** |
| 更新特性 | Source変更時に再生成 |
| 保持 | 再構築可能。保持期間は実装判断 |
| Transaction | 必須ではないが整合性管理が必要 |
| 再生成 | **可能** |

### Canonical Modelの考え方

固定しすぎない。

共通Envelopeのみ安定させる:

- source_id
- resource_id
- version_id
- title
- content
- language
- content_type
- created_at
- updated_at
- effective_from
- effective_to
- provenance
- locator
- access_scope
- metadata

Source固有項目はmetadataへ保持する。

---

## 5.3 KnowledgeUnit

| 特性 | 内容 |
|---|---|
| 役割 | Retrievalで扱う検索候補単位 |
| 正本性 | 派生 |
| 更新特性 | Extraction / Chunking変更時に再生成 |
| 保持 | 再生成可能 |
| 想定件数 | 元Resource数より大幅に増える可能性あり |
| 再生成 | **可能** |

単位例:

- 文書全体
- 章
- 節
- 段落
- 表
- Excel sheet
- ZIP内部ファイル

KnowledgeUnit粒度は検索精度に直接影響するため、評価対象とする。

---

## 5.4 Lexical Representation

| 特性 | 内容 |
|---|---|
| 用途 | BM25等の語彙検索 |
| 正本性 | 派生 |
| 更新 | KnowledgeUnit変更時に更新 |
| 保持 | 再生成可能 |
| 実装候補 | Tantivy + Lindera |
| 再生成 | **可能** |

位置情報・ハイライト用情報も保持可能にする。

---

## 5.5 Vector Representation

| 特性 | 内容 |
|---|---|
| 用途 | Semantic Retrieval |
| 正本性 | 派生 |
| 更新 | embedding model / chunk変更時に再生成 |
| 保持 | 再生成可能 |
| サイズ | embedding dimension × KnowledgeUnit数に依存 |
| 再生成 | **可能** |

### 注意

Embedding Modelの変更で全面再生成が発生し得るため、正本扱いしない。

---

## 5.6 Structured / Metadata Representation

| 特性 | 内容 |
|---|---|
| 用途 | department / type / law_number等のfilter・structured retrieval |
| 正本性 | 派生 |
| 更新 | Source metadata変更時 |
| 保持 | 再生成可能 |
| 再生成 | **可能** |

---

## 5.7 Temporal Representation

| 特性 | 内容 |
|---|---|
| 用途 | 時系列・有効期間・最新版判定 |
| 正本性 | 派生 |
| 更新 | Source側日時情報に追従 |
| 再生成 | **可能** |

単純な`updated_at`だけに潰さず、意味の異なる日時を維持する。

例:

- created_at
- published_at
- effective_from
- effective_to
- superseded_at

---

## 5.8 Graph Representation（将来）

| 特性 | 内容 |
|---|---|
| 用途 | 関係・参照・関連文書探索 |
| 正本性 | 派生 |
| 導入時期 | v0では必須としない |
| 再生成 | 可能であることを原則とする |

---

# 6. Query / Retrieval 実行データ

## 6.1 SearchRequest

| 特性 | 内容 |
|---|---|
| 役割 | 人間・LLM共通の検索要求 |
| 永続化 | 原則不要。監査要件に応じてAudit化 |
| 機密性 | 高くなり得る |

検索本文を通常ログへ無条件保存しない。

---

## 6.2 RetrievalPlan

| 特性 | 内容 |
|---|---|
| 役割 | QueryごとのRetriever・filter・fusion方式等の計画 |
| 生成 | rule / LLM Query Planner等 |
| 永続化 | デバッグ・評価用に識別子と設定を記録可能 |
| 再現性 | 重要 |

LLMが検索回数上限により打ち切られる設計にはしない。

Retrieval Planは必要に応じ再生成され、探索戦略を変更できる。

---

## 6.3 Candidate / Rank Trace

| 特性 | 内容 |
|---|---|
| 役割 | Retrieval → Fusion → Rerankingの追跡 |
| 正本性 | デバッグ・評価用 |
| 保持 | 保持期間を後続決定 |
| サイズ | 検索量次第で大きくなる可能性あり |

保持候補:

- retriever
- candidate resource/unit
- rank
- score
- fusion score
- rerank score
- model/version

検索精度問題をレイヤー単位で切り分けるために利用する。

---

# 7. Audit / Observability

## 7.1 AuditEvent

| 特性 | 内容 |
|---|---|
| 役割 | 誰が何をしたかの監査証跡 |
| 正本性 | **監査データとして正本** |
| 更新特性 | append-onlyを基本候補とする |
| 保持 | 長期。具体年限は後続要件 |
| Transaction | イベント欠損を避ける設計が必要 |
| 再生成 | 原則不可 |

例:

- document.create
- document.version.publish
- document.read
- search.execute
- search.result.open

---

## 7.2 OpenTelemetry Trace / Log / Metrics

| 特性 | 内容 |
|---|---|
| 役割 | 性能監視・障害調査・デバッグ |
| 正本性 | 運用観測データ |
| 保持 | 有限。保持期間を別途設定 |
| サイズ | 非常に増加しやすい |
| 再生成 | 不可だが永続保存必須ではない |

### 方針

- Document/Search/Extraction等は共通OpenTelemetry形式で出力する。
- Query本文や文書本文を通常ログへ無条件保存しない。
- AuditEventとは別系統として扱う。

---

# 8. ファイル形式の対応方針

「Windowsで使うすべての形式を全文検索可能にする」ことはv0の必須条件にしない。

## Tier 1: 内容まで検索対象

初期候補:

- DOC / DOCX
- XLS / XLSX
- PPT / PPTX
- PDF
- TXT
- CSV
- HTML
- ZIP

ZIP内のTier 1形式は可能な範囲で再帰Extractionする。

## Tier 2: 保存・版管理のみ

- その他Windows業務ファイル
- 未対応独自形式

## Tier 3: Plugin追加

必要性が判明した形式について `ContentExtractor` を追加する。

この方式により対応形式を将来的に増やせる構造を維持する。

---

# 9. データ配置の暫定責務

現時点では製品を確定せず、論理責務のみ固定する。

| データ | 配置責務 |
|---|---|
| Document / DocumentVersion | Transactional Metadata Store |
| Folder / Metadata | Transactional Metadata Store |
| ReadState | Transactional Metadata Store |
| AccessPolicy | Transactional Metadata Store |
| 実ファイル | File / Object Storage |
| Lexical Representation | Search Index |
| Vector Representation | Vector-capable Index / Store |
| Temporal / Structured Representation | Search Indexまたは派生Store |
| AuditEvent | Audit Store |
| Trace / Log / Metrics | Observability Platform |

---

# 10. DB / Storage選定への示唆

本書時点ではDB製品を確定しない。

ただし以下の要件は導出済みである。

## Transactional Metadata Storeに必要

- 約1,000ユーザー利用を前提
- 複数ユーザー同時アクセス
- Referential Integrity
- Unique Constraint
- `principal_id × document_version_id` の複合一意性
- version公開等でのtransaction
- 長期運用
- schema migration
- backup / restoreを後から強化可能
- 将来的な冗長化余地
- Rust driver / ecosystemの成熟度

## Search Storeに必要

- 正本から再構築可能
- KnowledgeUnit数の増加に対応
- lexical / vector / metadata / temporal検索への拡張
- Fusion / Rerankingの前段として十分なRecall
- index versioning / rebuild可能

## File Storageに必要

- 数百GB〜数TB以上へ増加する可能性
- 原則永久保存
- immutableな版ファイルとの相性
- hash等による整合性確認
- 将来のバックアップ方式変更に耐えられる

---

# 11. v0で未確定の項目

以下は実測・PoC・追加ヒアリングで更新する。

- 現在の正確なDocument件数
- 現在のDocumentVersion件数
- 平均Version数
- 年間文書増加件数
- 登録・改訂件数/日
- 人間のSearch QPS
- LLMのsearch_calls_per_turn分布
  - p50
  - p95
  - p99
  - max observed
- Retriever fan-out
- Index容量
- Embedding容量
- ピーク同時利用数
- SLA
- RPO / RTO
- Audit保持期間
- Telemetry保持期間

---

# 12. v0で固定する設計原則

1. **ReadStateは `user × document_version` で保持する。**
2. Document Platformは社内正式文書の正本を管理する。
3. Search Platformは派生データを管理し、正本から再構築可能にする。
4. 文書ファイル、Transactional Metadata、Search Index、Audit、Telemetryは論理的に分離する。
5. 検索のためのExtractionは登録・同期時のIndexing Pipelineで行う。
6. Human / LLM は同じDocument API / Search APIを利用する。
7. LLMの正常な検索回数には固定上限を設けない。
8. Query / Document本文を通常ログへ無条件保存しない。
9. SLA / Backup方式は後続で決定するが、後から強化できる構造を維持する。
10. DB / Storage製品は本データ特性から比較・選定する。

---

## 13. 次のアクション

本 Data Characteristics v0 を前提に、次に以下を行う。

1. Transactional Metadata Store候補比較
   - PostgreSQL
   - SQLite
   - その他Rustとの親和性が高い候補
2. File Storage方式比較
3. Search Index構成比較
4. 3〜5年の容量レンジ試算
5. PoC向けLoad / Evaluation Plan作成


# Observability/Audit Spec Repository Update Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Observability/Auditの横断仕様を追加し、既存Architecture/Bootstrap文書を新しい規範構造へ整合させる。

**Architecture:** `spec/`を規範SSOTとして維持し、ObservabilityはOpenTelemetry/OTLP、AuditはCloudEvents envelopeを用いる別責務として固定する。具体ライブラリ・backend選定は仕様確定後に行う。

**Tech Stack:** Markdown, D2。実装ライブラリ追加なし。

**Spec:** `spec/operations/observability-audit-requirements-v0.md`

## Global Constraints

- 顧客固有名詞を使用しない。
- ObservabilityとAuditを混同しない。
- Audit Eventはsamplingしない。
- Telemetry Backend/Audit Storeの具体製品を固定しない。
- ライブラリ選定より横断仕様更新を先行する。

---

### Task 1: Observability/Audit規範を配置する

**Files:**
- Create: `spec/operations/observability-audit-requirements-v0.md`

**Interfaces:**
- Consumes: Error Handling & Resilience Requirements v0
- Produces: Telemetry/Audit選定時の規範

- [ ] ファイルを配置する。
- [ ] 顧客固有名詞が存在しないことを確認する。
- [ ] OpenTelemetry / W3C Trace Context / CloudEventsの責務が分離されていることを確認する。
- [ ] Audit sampling禁止・Telemetry sampling許容が明文化されていることを確認する。

### Task 2: Architecture Contractへ横断標準を接続する

**Files:**
- Modify: `spec/architecture/architecture-contract-v0.md`

**Interfaces:**
- Consumes: Task 1
- Produces: 横断仕様へのnormative reference

- [ ] Error Handling / Observability & Auditを下位規範として追記する。
- [ ] OpenAPI 3.2.1 / JSON Schema 2020-12 / RFC 9457 / W3C Trace Context / OpenTelemetry / CloudEventsを列挙する。
- [ ] backend製品への直接依存禁止を確認する。

### Task 3: Bootstrap文書のrepo構造を更新する

**Files:**
- Modify: `docs/design/repository-bootstrap-design-v0.md`

**Interfaces:**
- Consumes: Task 1-2
- Produces: 実装前のリポジトリ構造・作業順

- [ ] `spec/operations/` を追加する。
- [ ] 横断仕様→Frontend/CI→Library Selection→PoCの順序を追加する。
- [ ] 実装ライブラリをまだ追加しない方針を確認する。

### Task 4: リポジトリ更新を検証する

**Files:**
- Inspect: `spec/**/*.md`
- Inspect: `docs/design/**/*.md`

**Interfaces:**
- Consumes: Task 1-3
- Produces: repository-ready update

- [ ] 顧客固有名詞が含まれていないことを検索する。
- [ ] 規範文書間の名称・versionが整合することを確認する。
- [ ] ライブラリ選定の具体実装が混入していないことを確認する。
- [ ] GitHubへ反映後、exact HEADとCI結果を記録する。

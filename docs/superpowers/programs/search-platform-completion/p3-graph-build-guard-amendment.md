# P3 Durable HyperGraph — incremental build guard 追補

Status: **DRAFT / 独立再審査待ち**（2026-09-30）。`p3-graph-design-revision-1.md` §3–5/§7 のうち、`p3-graph-architecture-recheck.md` の残る P2 一件だけを補う。抵触する build / GC 手順には本追補を優先する。既報 P2-2〜5 の意味論、PostgreSQL の暫定第一候補という位置付け、P3 と P7 の責務境界は変えない。

## 拘束する不変条件と SQL 形

`stage_incremental` の一回の試行は、一つの Source、既存 base READY key、新規 target BUILDING key、検証済み base receipt、target manifest、推測不能な `guard_token`、Source 内単調増加の `build_fence`、DB 時刻の `expires_at` に束縛する。guard は evaluation lease と別物で、**base と target の両 key**を保護する。現在 pointer が別 generation に進んでも guard は有効なままである。`pointer_revision` の変化を copy 失敗条件にせず、毎 batch で現在の **build fence** を確認する。

PostgreSQL 候補の必須 schema 差分は次の形とする。P7 所有 `source_control` に `build_fence_seq BIGINT NOT NULL DEFAULT 0 CHECK (build_fence_seq >= 0)` を追加し、Source row lock 下の登録時だけ overflow を拒否して増分する。既存 `generation` の `(source_id,generation_id)` は UUID 複合 PK とする。

```sql
ALTER TABLE search_graph.generation
  ADD COLUMN incremental_base_generation_id uuid,
  ADD COLUMN build_guard_token uuid,
  ADD COLUMN build_fence bigint,
  ADD CONSTRAINT incremental_binding_complete CHECK (
    (incremental_base_generation_id IS NULL AND build_guard_token IS NULL AND build_fence IS NULL)
    OR (incremental_base_generation_id IS NOT NULL AND build_guard_token IS NOT NULL
        AND build_fence IS NOT NULL AND build_fence > 0)
  ),
  ADD CONSTRAINT generation_build_binding_unique
    UNIQUE (source_id, generation_id, build_guard_token, build_fence);

CREATE TABLE search_graph.build_guard (
  source_id uuid NOT NULL,
  base_generation_id uuid NOT NULL,
  target_generation_id uuid NOT NULL,
  guard_token uuid NOT NULL UNIQUE,
  fence bigint NOT NULL CHECK (fence > 0),
  base_manifest_digest text NOT NULL,
  base_graph_content_digest text NOT NULL,
  base_source_snapshot text NOT NULL,
  base_source_mapping_digest text NOT NULL,
  base_resource_count bigint NOT NULL CHECK (base_resource_count >= 0),
  base_relation_count bigint NOT NULL CHECK (base_relation_count >= 0),
  target_manifest_digest text NOT NULL,
  expires_at timestamptz NOT NULL,
  copy_verified_at timestamptz,
  PRIMARY KEY (source_id, target_generation_id),
  UNIQUE (source_id, fence),
  CHECK (base_generation_id <> target_generation_id),
  FOREIGN KEY (source_id, base_generation_id)
    REFERENCES search_graph.generation (source_id, generation_id) ON DELETE RESTRICT,
  FOREIGN KEY (source_id, target_generation_id, guard_token, fence)
    REFERENCES search_graph.generation
      (source_id, generation_id, build_guard_token, build_fence) ON DELETE RESTRICT
);
CREATE INDEX build_guard_by_base
  ON search_graph.build_guard (source_id, base_generation_id);
CREATE INDEX build_guard_by_expiry
  ON search_graph.build_guard (expires_at, source_id, target_generation_id);
```

行間の READY / BUILDING、receipt 一致、`expires_at > clock_timestamp()` は `CHECK` に置かず、下記 transaction と既存 DB mutation fence を拡張した trigger / DB role で強制する。incremental target の子表 DML は親 BUILDING lock に加え、同一 token / fence の有効 guard を確認し、失効後の書込みを拒否する。guard と target の token / fence は登録後不変とし、外部 request や通常 builder に guard 行の直接 DML 権限を与えない。`ON DELETE RESTRICT` は guard を残した base / target の物理削除も防ぐ。

## 共有 transaction / port の追補

P7 と P3 の共有 lock 順は **source control `FOR UPDATE` → 対象 generation を key 順（base は `FOR SHARE`、target は `FOR UPDATE`）→ guard を `(source_id,target_generation_id)` 順 → evaluation lease を ID 順**とする。copy / delta / `validate_ready` は Source lock を取らず、generation lock の後から Source lock を要求しない。P7 の登録、renew、publish、abort、GC、期限切れ cleanup は Source lock から始める。lock wait / deadlock / serialization error は改訂 §5 の transaction 全体 rollback と bounded retry に従う。

1. `stage_incremental` は最初の copy batch **より前**に一つの transaction で Source row を lock し、base と新規 target の generation key を整列して lock する。target 行はこの未 commit transaction 内で初めて作り、外部から見える無 guard BUILDING 行を作らない。既存 generation の競合 lock は key 順に取得し、新規 target の INSERT lock は commit 前に他者から取得できない。base の READY、manifest / snapshot / mapping / content digest / counts を receipt と一致させ、target の新規 key、BUILDING、target manifest を確認する。同じ transaction で `build_fence_seq` を増分し、target binding と guard を INSERT して commit する。base が既に消えた、guard を登録できない、Source control がない場合は copy を始めず full rebuild に切り替え、それも不能なら READY にしない。
2. `BuildGuardHandle { source, base_key, target_key, token, fence }` は信頼済み内部 port の opaque 値とし、SQLx 型や公開 request に出さない。`stage_incremental` は target key と handle を返す。`renew_build_guard(handle, bounded_ttl)` は Source→generation→guard lock の transaction で token / fence / 両 key と未失効を再確認し、DB clock から expiry を延長する。失効済み guard は復活させず、同じ target へ新 token を発行しない。TTL 上限と renewal cadence は P7 plan で固定する。
3. **各 copy batch** は base `FOR SHARE` と target `FOR UPDATE` を key 順に transaction 終了まで保持し、guard 行を lock した上で handle と target binding の token / fence、`expires_at > clock_timestamp()`、base READY と凍結 receipt の digest / manifest / counts、target BUILDING / manifest、`copy_verified_at IS NULL` を再確認してから読み書きする。base の全行と relation ID は同じ不変 READY receipt に属する。partial copy の続行位置は commit 済み batch cursor のみとし、検証不能なら target を再利用しない。copy 完了時は同じ locks の下で **delta 適用前**の target 全行の count / canonical content digest を base receipt と照合し、成功時だけ `copy_verified_at` を記録する。delta batch も同じ token / fence / expiry / base receipt / target BUILDING の gate と、`copy_verified_at IS NOT NULL` を通す。各 batch は commit 直前にも DB clock の expiry を確認し、失効を検知した transaction は rollback する。`validate_ready` も両 generation を key 順に lock して有効 guard と copy verification を確認してから改訂 §4 の READY 検証・遷移を行い、guard はまだ解除しない。
4. `retire_unpinned`、`discard_unpublished`、BUILDING cleanup は、current / evaluation lease に加え、対象 key が **base または target** の有効 guard を持つなら DELETING と物理削除を拒否する。Source lock 下で generation と該当 guard を lock して DB clock で判定する。期限切れ guard を見た GC は先に次項の cleanup を完了してから再判定し、単に guard を無視して base を削除しない。P7 未接続の isolated P3 は改訂 §5 どおり READY の退役 port を公開せず、BUILDING cleanup は隔離 fixture の非公開条件に限る。
5. `publish_if_current(..., handle)` は Source→base / target generation key 順→guard→lease 順の一 transaction で base READY / 凍結 receipt、target READY / receipt と未失効 token / fence を再確認し、pointer CAS 成功と guard DELETE を**同じ commit**で確定する。READY 遷移だけでは解除しないので、READY から publish まで target を GC できる窓はない。CAS 敗北なら pointer を変更せず guard を保持して明示 abort へ渡す。`abort_build(handle)` は同じ lock / current / lease 確認の transaction で guard を削除し、未公開 target を DELETING へ遷移して子行・親行を削除する。base の保護解除と target の廃棄は同じ commit で可視化する。公開済み target や有効 evaluation lease のある target は abort で削除しない。
6. crash / restart 後の bounded cleanup は Source→両 generation key 順→guard の下で DB clock の expiry を再判定する。失効 guard の target が未公開かつ未 pin なら、BUILDING / 未公開 READY target を同一 transaction で terminal 化・削除し、その guard を削除する。未失効 guard は勝手に削除しない。期限切れを観測した旧 handle の renew、copy、delta、READY 化、publish はすべて fail closed とし、base が既に退役した場合を含め target を継続しない。期限到達直前から実行中の batch も generation locks を commit まで保持するため、base 削除後の copy は起こらない。想定外の current / pin と guard の共存は削除せず integrity failure として調査する。次の試行は別 target と新 fence で full rebuild、または検証可能な READY base から新規 incremental とする。

## 実 DB の一つの競合 schedule

barrier 付き試験: A を current READY、T を A からの guarded BUILDING とし、T の copy batch 1 を commit する。別 publisher が C を current に切り替え、A の evaluation lease がない状態で、`retire_unpinned(A)`、`BUILDING cleanup(T)`、T の copy batch 2 を競合させる。GC が先に Source lock を得ても両 key の有効 guard により A / T の削除を拒否する。copy が先に generation locks を得た場合は GC が待って再判定し、batch 2 は A の凍結 digest に対して完了する。続けて TTL 失効と crash cleanup を注入し、T の terminal cleanup 後は stale handle の batch / READY / publish が失敗し、A の GC が初めて可能になることを確認する。別の成功枝は A が current の時点から開始し、T を READY にして expected=A の publish CAS と guard DELETE を同一 commit で行い、直後の GC が current T を保持することを確認する。各枝で row / digest / pointer を実 DB で照合し、SD-T1 / SD-T6 の full↔incremental 論理等価を維持する。

本追補は設計条件であり、migration、trigger、並行試験、PoC、backend 選定、P7 接続の達成を示さない。PostgreSQL の row lock、DB 時刻、FK `RESTRICT` の性質は [公式 explicit locking](https://www.postgresql.org/docs/current/explicit-locking.html)、[date/time functions](https://www.postgresql.org/docs/current/functions-datetime.html)、[constraints](https://www.postgresql.org/docs/current/ddl-constraints.html) を根拠とする。

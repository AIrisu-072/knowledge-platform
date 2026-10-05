-- 専用の非所有者・非特権ロールに適用するテンプレート。
-- 他ロールの継承権限や PUBLIC の追加権限は与えない構成を前提とする。
-- 管理者が DELIVERY_ROLE を検証済みのロール名に置換する。
REVOKE ALL PRIVILEGES ON TABLE
    outbox_events, outbox_delivery_policy, documents, audit_outbox_events
    FROM "{{DELIVERY_ROLE}}";
GRANT USAGE ON SCHEMA public TO "{{DELIVERY_ROLE}}";
GRANT SELECT ON TABLE outbox_events, outbox_delivery_policy TO "{{DELIVERY_ROLE}}";
GRANT UPDATE (
    attempt_count, attempt_limit, available_at,
    lease_token, lease_owner, lease_expires_at, last_attempt_at,
    delivered_at, dead_lettered_at, last_error_code
) ON TABLE outbox_events TO "{{DELIVERY_ROLE}}";
-- FOR SHARE が必要とする唯一の UPDATE 権限。PK/NOT NULL/CHECK(policy_id=1)
-- により値を変えない代入だけが可能で、方針値を編集する権限ではない。
GRANT UPDATE (policy_id) ON TABLE outbox_delivery_policy TO "{{DELIVERY_ROLE}}";

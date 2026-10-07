//! Role matrix, posture, search_path shadowing, principal binding,
//! bootstrap, capabilities and the two-phase disclosure (design §7.3, §10).

mod support;

use std::collections::BTreeSet;
use std::time::Duration;

use audit_core::AuditStore;
use audit_store_postgres::AdminError;
use audit_store_postgres::PostgresAuditStore;
use audit_store_postgres::admin::{AccessChange, AccessOperation};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection, PgPool, Row};
use support::*;
use uuid::Uuid;

async fn ingest_documents(db: &TestDb, cast: &Cast, count: usize) -> Vec<Uuid> {
    let store = PostgresAuditStore::new(cast.relay.pool.clone(), Duration::from_secs(10))
        .await
        .expect("relay");
    let mut ids = Vec::new();
    for n in 0..count {
        let id = Uuid::now_v7();
        let envelope = if n % 2 == 0 {
            document_created(
                id,
                Uuid::now_v7(),
                OCCURRED,
                u8::try_from(n % 250).expect("u8"),
            )
        } else {
            read_confirmed(
                id,
                Uuid::now_v7(),
                OCCURRED,
                u8::try_from(n % 250).expect("u8"),
            )
        };
        store.ingest(&envelope).await.expect("stored");
        ids.push(id);
    }
    let _ = db;
    ids
}

fn denied(code: &str) -> AdminError {
    AdminError::Denied { code: code.into() }
}

fn rejected(code: &str) -> AdminError {
    AdminError::Rejected { code: code.into() }
}

/// Every public function, called with harmless arguments, and the
/// capability roles allowed to EXECUTE it (design §10.1 plus status/posture).
const CALLS: &[(&str, &[&str])] = &[
    (
        "SELECT * FROM audit_store.ingest('{}'::jsonb)",
        &["audit_store_ingest"],
    ),
    ("SELECT * FROM audit_store.probe()", &["audit_store_ingest"]),
    (
        "SELECT * FROM audit_store.lookup_receipts(ARRAY[]::uuid[])",
        &["audit_store_ingest"],
    ),
    (
        "SELECT * FROM audit_store.list_source_receipts('x', 0, 1)",
        &["audit_store_ingest"],
    ),
    (
        "SELECT * FROM audit_store.lookup_control_receipts(ARRAY[]::bigint[])",
        &["audit_store_ingest"],
    ),
    (
        "SELECT * FROM audit_store.record_relay_control('x', NULL, 'x', NULL)",
        &["audit_store_relay_control"],
    ),
    (
        "SELECT * FROM audit_store.open_access('investigate', '{}', 1, 1)",
        &["audit_store_reader", "audit_store_verifier"],
    ),
    (
        "SELECT * FROM audit_store.read_page(repeat('0', 64), 0)",
        &["audit_store_reader", "audit_store_verifier"],
    ),
    (
        "SELECT * FROM audit_store.close_access(repeat('0', 64), 0, ARRAY[]::text[])",
        &["audit_store_reader", "audit_store_verifier"],
    ),
    (
        "SELECT * FROM audit_store.verify(NULL, NULL)",
        &["audit_store_verifier"],
    ),
    (
        "SELECT * FROM audit_store.checkpoint()",
        &["audit_store_verifier"],
    ),
    (
        "SELECT * FROM audit_store.verify_recovery()",
        &["audit_store_verifier", "audit_store_maintainer"],
    ),
    (
        "SELECT * FROM audit_store.identity_chain_recovery_page(0, 1)",
        &["audit_store_verifier", "audit_store_maintainer"],
    ),
    (
        "SELECT * FROM audit_store.change_access('a', 'b', 'verify', 'grant')",
        &["audit_store_admin"],
    ),
    (
        "SELECT * FROM audit_store.set_retention_policy('p', '{}', NULL)",
        &["audit_store_admin"],
    ),
    (
        "SELECT * FROM audit_store.expire('p', 1, now(), 1)",
        &["audit_store_maintainer"],
    ),
    (
        "SELECT * FROM audit_store.purge_body(gen_random_uuid(), 'adapter_defect')",
        &["audit_store_maintainer"],
    ),
    (
        "SELECT * FROM audit_store.begin_recovery_epoch(1, 0, repeat('0', 64), 0)",
        &["audit_store_maintainer"],
    ),
    (
        "SELECT * FROM audit_store.rebind_fingerprint()",
        &["audit_store_maintainer"],
    ),
    (
        "SELECT * FROM audit_store.store_status()",
        &[
            "audit_store_ingest",
            "audit_store_relay_control",
            "audit_store_reader",
            "audit_store_verifier",
            "audit_store_admin",
            "audit_store_maintainer",
        ],
    ),
    (
        "SELECT * FROM audit_store.posture_check()",
        &[
            "audit_store_verifier",
            "audit_store_admin",
            "audit_store_maintainer",
        ],
    ),
    // Owner members only.
    (
        "SELECT * FROM audit_store.bootstrap_administrator('x', 'a', 'b')",
        &[],
    ),
    (
        "SELECT * FROM audit_store.bind_principal('x', 'a', 'b')",
        &[],
    ),
    ("SELECT * FROM audit_store.unbind_principal('x')", &[]),
    // Internal functions: nobody.
    (
        "SELECT audit_store.append_control('store', 'audit.access.denied', 'SECURITY', 'denied', '{}')",
        &[],
    ),
    (
        "SELECT audit_store.record_denied('expire', 'unbound', NULL)",
        &[],
    ),
    ("SELECT * FROM audit_store.integrity_scan(1, 0, FALSE)", &[]),
    ("SELECT audit_store.lock_head()", &[]),
    (
        "SELECT * FROM audit_store.authorize('expire', 'audit_store_maintainer', 'maintain')",
        &[],
    ),
    ("SELECT audit_store.resolve_intent(repeat('0', 64))", &[]),
    (
        "SELECT audit_store.gate_code(NULL::audit_store.publication_head)",
        &[],
    ),
];

async fn call(pool: &PgPool, sql: &'static str) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let result = sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&mut *tx)
        .await
        .map(|_| ());
    tx.rollback().await?;
    result
}

fn is_execute_denied(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db)
        if db.code().as_deref() == Some("42501")
            && db.message().starts_with("permission denied for function"))
}

#[tokio::test]
async fn role_matrix_refuses_every_function_outside_the_role() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let logins = [
        (
            "audit_store_ingest",
            db.login("only_ingest", &["audit_store_ingest"]).await,
        ),
        (
            "audit_store_relay_control",
            db.login("only_relay_control", &["audit_store_relay_control"])
                .await,
        ),
        (
            "audit_store_reader",
            db.login("only_reader", &["audit_store_reader"]).await,
        ),
        (
            "audit_store_verifier",
            db.login("only_verifier", &["audit_store_verifier"]).await,
        ),
        (
            "audit_store_admin",
            db.login("only_admin", &["audit_store_admin"]).await,
        ),
        (
            "audit_store_maintainer",
            db.login("only_maintainer", &["audit_store_maintainer"])
                .await,
        ),
    ];
    // A login without any capability role cannot even connect: CONNECT is
    // revoked from PUBLIC (sql/privileges.sql).
    let nobody = db.role_name("nobody");
    db.exec(&format!("CREATE ROLE {nobody} LOGIN PASSWORD '{PASSWORD}'"))
        .await;
    let error = PgConnection::connect(&db.url(&nobody, &db.database))
        .await
        .expect_err("no CONNECT");
    assert_eq!(sqlstate(&error), "42501");
    for (role, login) in &logins {
        for (sql, allowed) in CALLS {
            let result = call(&login.pool, sql).await;
            let refused = result.as_ref().is_err_and(is_execute_denied);
            if allowed.contains(role) {
                assert!(!refused, "{role} must be allowed: {sql}");
            } else {
                assert!(refused, "{role} must get 42501: {sql} -> {result:?}");
            }
        }
    }
    // Owner members may call the owner functions (the body decides).
    for sql in [
        "SELECT * FROM audit_store.unbind_principal('x')",
        "SELECT * FROM audit_store.bind_principal('x', 'a', 'b')",
    ] {
        assert!(call(&cast.dba.pool, sql).await.is_ok(), "{sql}");
    }
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn posture_is_clean_and_detects_each_violation() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let verifier = cast.verifier.admin().await;
    assert_eq!(verifier.posture().await.expect("posture"), vec![]);
    let reader = cast.reader.admin().await;
    let scenarios: &[(&str, &str, &str)] = &[
        (
            "GRANT EXECUTE ON FUNCTION audit_store.read_page(text, bigint) TO PUBLIC",
            "REVOKE EXECUTE ON FUNCTION audit_store.read_page(text, bigint) FROM PUBLIC",
            "public_execute",
        ),
        (
            "GRANT EXECUTE ON FUNCTION audit_store.verify(bigint, bigint) TO audit_store_reader",
            "REVOKE EXECUTE ON FUNCTION audit_store.verify(bigint, bigint) FROM audit_store_reader",
            "unexpected_execute",
        ),
        (
            "REVOKE EXECUTE ON FUNCTION audit_store.ingest(jsonb) FROM audit_store_ingest",
            "GRANT EXECUTE ON FUNCTION audit_store.ingest(jsonb) TO audit_store_ingest",
            "missing_execute",
        ),
        (
            "GRANT CONNECT ON DATABASE audit_store_test TO PUBLIC",
            "REVOKE CONNECT ON DATABASE audit_store_test FROM PUBLIC",
            "database_public_connect",
        ),
        (
            "ALTER FUNCTION audit_store.probe() RESET search_path",
            "ALTER FUNCTION audit_store.probe() SET search_path = pg_catalog, pg_temp",
            "search_path_unpinned",
        ),
        (
            "ALTER FUNCTION audit_store.probe() OWNER TO postgres",
            "ALTER FUNCTION audit_store.probe() OWNER TO audit_store_owner",
            "function_owner",
        ),
        (
            "GRANT SELECT ON audit_store.events TO audit_store_reader",
            "REVOKE SELECT ON audit_store.events FROM audit_store_reader",
            "relation_privilege",
        ),
        (
            "GRANT USAGE ON SCHEMA audit_store TO PUBLIC",
            "REVOKE USAGE ON SCHEMA audit_store FROM PUBLIC",
            "schema_usage",
        ),
        (
            "ALTER DEFAULT PRIVILEGES FOR ROLE audit_store_owner GRANT EXECUTE ON FUNCTIONS TO PUBLIC",
            "ALTER DEFAULT PRIVILEGES FOR ROLE audit_store_owner REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC",
            "default_execute_not_revoked",
        ),
    ];
    if db.container.is_none() {
        // External servers use a generated database name.
        return;
    }
    for (break_sql, repair_sql, violation) in scenarios {
        db.exec(break_sql).await;
        let found: BTreeSet<String> = verifier
            .posture()
            .await
            .expect("posture")
            .into_iter()
            .map(|v| v.violation)
            .collect();
        assert!(found.contains(*violation), "{violation}: {found:?}");
        // Publication is fail-closed while the posture is invalid.
        if *violation != "missing_execute" {
            assert_eq!(
                reader
                    .open_access(AccessOperation::Export, &json!({}), 10, 1)
                    .await
                    .expect_err(violation),
                AdminError::PostureInvalid
            );
        }
        db.exec(repair_sql).await;
        assert_eq!(
            verifier.posture().await.expect("posture"),
            vec![],
            "{violation}"
        );
    }
    // A login granted a capability without re-running privileges.sql.
    let role = db.role_name("late");
    db.exec(&format!(
        "CREATE ROLE {role} LOGIN PASSWORD '{PASSWORD}'; GRANT audit_store_reader TO {role}"
    ))
    .await;
    let found = verifier.posture().await.expect("posture");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].violation, "login_timeouts_missing");
    assert_eq!(found[0].object, role);
    db.exec(audit_store_postgres::PRIVILEGES_SQL).await;
    assert_eq!(verifier.posture().await.expect("posture"), vec![]);
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn temp_objects_cannot_shadow_store_objects() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    ingest_documents(&db, &cast, 3).await;
    // TEMPORARY is revoked from PUBLIC: the first attempt fails outright.
    let error = sqlx::query("CREATE TEMP TABLE publication_head (last_seq bigint)")
        .execute(&cast.verifier.pool)
        .await
        .expect_err("no TEMP privilege");
    assert_eq!(sqlstate(&error), "42501");

    // Even with TEMP granted, pinned search_path and qualified names win.
    let unbound = db.login("shadow", &["audit_store_reader"]).await;
    db.exec(&format!(
        "GRANT TEMPORARY ON DATABASE {} TO {}, {}",
        db.database, cast.verifier.role, unbound.role
    ))
    .await;
    let mut conn = cast.verifier.pool.acquire().await.expect("conn");
    for statement in [
        "CREATE TEMP TABLE publication_head (singleton boolean, last_seq bigint, last_chain bytea)",
        "INSERT INTO pg_temp.publication_head VALUES (true, 999, '\\x00')",
        "CREATE TEMP TABLE events (seq bigint)",
        "CREATE TEMP TABLE access_grants (issuer text, principal_id text, capability text, revoked_seq bigint)",
        "CREATE FUNCTION pg_temp.sha256(bytea) RETURNS bytea LANGUAGE sql AS $$ SELECT '\\x00'::bytea $$",
        "CREATE FUNCTION pg_temp.pg_has_role(name, text, text) RETURNS boolean LANGUAGE sql AS $$ SELECT true $$",
        "SET search_path = pg_temp, audit_store, public",
    ] {
        sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
            .execute(&mut *conn)
            .await
            .unwrap_or_else(|e| panic!("{statement}: {e}"));
    }
    let row =
        sqlx::query("SELECT status, outcome, checked, to_seq FROM audit_store.verify(NULL, NULL)")
            .fetch_one(&mut *conn)
            .await
            .expect("verify through shadowing session");
    assert_eq!(row.get::<String, _>("outcome"), "ok");
    let to_seq: i64 = row.get("to_seq");
    assert!(to_seq > 3 && to_seq < 999);
    drop(conn);

    let mut conn = unbound.pool.acquire().await.expect("conn");
    for statement in [
        "CREATE TEMP TABLE principal_bindings (bound_seq bigint, db_role text, issuer text, principal_id text, unbound_seq bigint)",
        "SET search_path = pg_temp, audit_store, public",
    ] {
        sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
            .execute(&mut *conn)
            .await
            .expect("shadow setup");
    }
    sqlx::query("INSERT INTO pg_temp.principal_bindings VALUES (1, $1, $2, 'reader-1', NULL)")
        .bind(&unbound.role)
        .bind(ISSUER)
        .execute(&mut *conn)
        .await
        .expect("fake binding");
    let row =
        sqlx::query("SELECT status, code FROM audit_store.open_access('export', '{}', 10, 1)")
            .fetch_one(&mut *conn)
            .await
            .expect("open_access");
    assert_eq!(
        (
            row.get::<String, _>("status"),
            row.get::<Option<String>, _>("code")
        ),
        ("denied".to_owned(), Some("unbound".to_owned()))
    );
    drop(conn);
    db.exec(&format!(
        "REVOKE TEMPORARY ON DATABASE {} FROM {}, {}",
        db.database, cast.verifier.role, unbound.role
    ))
    .await;
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn unbound_and_unauthorized_principals_are_denied_and_recorded() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let unbound = db.login("stranger", &["audit_store_reader"]).await;
    assert_eq!(
        unbound
            .admin()
            .await
            .open_access(AccessOperation::Investigate, &json!({}), 10, 1)
            .await
            .expect_err("unbound"),
        denied("unbound")
    );
    // Bound without the Audit capability.
    let verifier2 = db.login("verifier2", &["audit_store_verifier"]).await;
    cast.dba
        .owner()
        .await
        .bind_principal(&verifier2.role, ISSUER, "verifier-2")
        .await
        .expect("bind");
    assert_eq!(
        verifier2
            .admin()
            .await
            .verify(None, None)
            .await
            .expect_err("no capability"),
        denied("insufficient_capability")
    );
    // The reader holds investigate but not the verifier DB role.
    assert_eq!(
        cast.reader
            .admin()
            .await
            .open_access(AccessOperation::Verify, &json!({}), 10, 1)
            .await
            .expect_err("no role"),
        denied("insufficient_capability")
    );
    // The administrator holds the reader DB role but not investigate.
    assert_eq!(
        cast.admin
            .admin()
            .await
            .open_access(AccessOperation::Investigate, &json!({}), 10, 1)
            .await
            .expect_err("no capability"),
        denied("insufficient_capability")
    );
    // Invalid input is recorded with a bounded shape only.
    let reader = cast.reader.admin().await;
    for (filter, page_size) in [
        (json!({"free_text": "secret-value"}), 10),
        (json!({"event_types": "document.created"}), 10),
        (json!({"event_ids": ["not-a-uuid"]}), 10),
        (json!({}), 0),
        (json!({}), 101),
    ] {
        assert_eq!(
            reader
                .open_access(AccessOperation::Investigate, &filter, page_size, 1)
                .await
                .expect_err("invalid"),
            denied("invalid_input")
        );
    }
    let denials = control_events(&db.admin, "audit.access.denied").await;
    let codes: Vec<&str> = denials
        .iter()
        .map(|(_, d)| d["denial_code"].as_str().expect("code"))
        .collect();
    assert_eq!(
        codes,
        vec![
            "unbound",
            "insufficient_capability",
            "insufficient_capability",
            "insufficient_capability",
            "invalid_input",
            "invalid_input",
            "invalid_input",
            "invalid_input",
            "invalid_input"
        ]
    );
    for (_, details) in &denials {
        let keys: BTreeSet<&str> = details
            .as_object()
            .expect("details")
            .keys()
            .map(String::as_str)
            .collect();
        assert!(
            keys.is_subset(&BTreeSet::from([
                "session_role",
                "operation",
                "denial_code",
                "required_capability"
            ])),
            "{keys:?}"
        );
        assert!(!details.to_string().contains("secret-value"));
    }
    assert_eq!(denials[0].1["session_role"], json!(unbound.role));
    let actor: Value = sqlx::query(
        "SELECT b.envelope -> 'data' -> 'actor' AS actor FROM audit_store.event_bodies AS b \
         WHERE b.seq = $1",
    )
    .bind(denials[0].0)
    .fetch_one(&db.admin)
    .await
    .expect("actor")
    .get("actor");
    assert_eq!(
        actor,
        json!({"issuer": "audit-store-db-role", "principal_id": unbound.role})
    );
    let intents: i64 = sqlx::query("SELECT count(*) AS n FROM audit_store.access_intents")
        .fetch_one(&db.admin)
        .await
        .expect("intents")
        .get("n");
    assert_eq!(intents, 0, "nothing was opened");
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn self_grant_is_refused_and_bind_unbind_are_owner_only() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let admin = cast.admin.admin().await;
    assert_eq!(
        admin
            .change_access(ISSUER, "admin-1", "investigate", AccessChange::Grant)
            .await
            .expect_err("self grant"),
        denied("self_grant")
    );
    let denial = control_events(&db.admin, "audit.access.denied").await;
    assert_eq!(
        denial.last().expect("denied").1["denial_code"],
        json!("self_grant")
    );
    // The other administrator may grant it.
    cast.admin2
        .admin()
        .await
        .change_access(ISSUER, "admin-1", "investigate", AccessChange::Grant)
        .await
        .expect("granted by another administrator");
    assert_eq!(
        admin
            .change_access(ISSUER, "reader-1", "investigate", AccessChange::Grant)
            .await
            .expect("idempotent")
            .status,
        "unchanged"
    );

    // Only owner members bind; administrators cannot change who they are.
    for pool in [&cast.admin.pool, &cast.reader.pool, &cast.maintainer.pool] {
        let error = sqlx::query("SELECT * FROM audit_store.bind_principal($1, 'x', 'y')")
            .bind(&cast.reader.role)
            .fetch_one(pool)
            .await
            .expect_err("not an owner member");
        assert!(is_execute_denied(&error), "{error}");
    }
    let owner = cast.dba.owner().await;
    assert_eq!(
        owner
            .bind_principal(&cast.reader.role, ISSUER, "someone-else")
            .await
            .expect_err("bindings are not overwritten"),
        denied("already_bound")
    );
    assert_eq!(
        owner
            .bind_principal(&cast.dba.role, ISSUER, "dba")
            .await
            .expect_err("owner members are not bindable"),
        denied("invalid_input")
    );
    assert_eq!(
        owner
            .bind_principal("postgres", ISSUER, "superuser")
            .await
            .expect_err("superusers are not bindable"),
        denied("invalid_input")
    );
    owner
        .unbind_principal(&cast.reader.role)
        .await
        .expect("unbind");
    owner
        .bind_principal(&cast.reader.role, ISSUER, "reader-renamed")
        .await
        .expect("rebind after unbind");
    let history = sqlx::query(
        "SELECT principal_id, unbound_seq IS NOT NULL AS ended \
         FROM audit_store.principal_bindings WHERE db_role = $1 ORDER BY bound_seq",
    )
    .bind(&cast.reader.role)
    .fetch_all(&db.admin)
    .await
    .expect("history");
    assert_eq!(
        history
            .iter()
            .map(|r| (
                r.get::<String, _>("principal_id"),
                r.get::<bool, _>("ended")
            ))
            .collect::<Vec<_>>(),
        vec![
            ("reader-1".to_owned(), true),
            ("reader-renamed".to_owned(), false)
        ]
    );
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn bootstrap_is_once_even_when_concurrent_and_recovers_lockout() {
    let db = TestDb::start().await;
    let dba1 = db.owner_login("dba1").await;
    let dba2 = db.owner_login("dba2").await;
    let first = db.login("first", &["audit_store_admin"]).await;
    let second = db.login("second", &["audit_store_admin"]).await;
    let (a, b) = tokio::join!(
        async {
            dba1.owner()
                .await
                .bootstrap_administrator(&first.role, ISSUER, "first-admin")
                .await
        },
        async {
            dba2.owner()
                .await
                .bootstrap_administrator(&second.role, ISSUER, "second-admin")
                .await
        }
    );
    let outcomes = [a.is_ok(), b.is_ok()];
    assert_eq!(outcomes.iter().filter(|ok| **ok).count(), 1, "{a:?} {b:?}");
    let loser = if a.is_ok() { b } else { a };
    assert_eq!(loser, Err(denied("administrator_exists")));
    let administrators: i64 = sqlx::query(
        "SELECT count(*) AS n FROM audit_store.access_grants \
         WHERE capability = 'administer' AND revoked_seq IS NULL",
    )
    .fetch_one(&db.admin)
    .await
    .expect("count")
    .get("n");
    assert_eq!(administrators, 1);
    // Lockout recovery: the only administrator revokes itself, then the
    // same audited path bootstraps again.
    let (winner, principal) = if outcomes[0] {
        (&first, "first-admin")
    } else {
        (&second, "second-admin")
    };
    winner
        .admin()
        .await
        .change_access(ISSUER, principal, "administer", AccessChange::Revoke)
        .await
        .expect("self revoke");
    let other = db.login("third", &["audit_store_admin"]).await;
    dba1.owner()
        .await
        .bootstrap_administrator(&other.role, ISSUER, "third-admin")
        .await
        .expect("lockout recovery");
    let bootstraps = control_events(&db.admin, "audit.access_policy.changed")
        .await
        .into_iter()
        .filter(|(_, d)| d["change"] == "bootstrap")
        .count();
    assert_eq!(bootstraps, 2);
    db.assert_store_conforms().await;
}

/// Opens an export intent as the reader (autocommit) and returns the token.
async fn open_export(cast: &Cast) -> String {
    cast.reader
        .admin()
        .await
        .open_access(AccessOperation::Export, &json!({}), 10, 2)
        .await
        .expect("opened")
        .secret()
        .to_owned()
}

async fn read_in(conn: &mut PgConnection, token: &str) -> Result<i64, sqlx::Error> {
    sqlx::query("SELECT count(*) AS n FROM audit_store.read_page($1, 0)")
        .bind(token)
        .fetch_one(&mut *conn)
        .await
        .map(|row| row.get("n"))
}

fn refusal(result: Result<i64, sqlx::Error>) -> String {
    match result.expect_err("refused") {
        sqlx::Error::Database(db) => {
            assert_eq!(db.code().as_deref(), Some("42501"));
            db.message().to_owned()
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn read_page_needs_a_committed_intent_and_a_clean_transaction() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    ingest_documents(&db, &cast, 5).await;
    let mut conn = PgConnection::connect(&cast.reader.url)
        .await
        .expect("connect");
    let open = "SELECT token FROM audit_store.open_access('export', '{}', 10, 2)";

    // open_access then read_page in the same transaction.
    sqlx::query("BEGIN")
        .execute(&mut conn)
        .await
        .expect("begin");
    let token: String = sqlx::query(open)
        .fetch_one(&mut conn)
        .await
        .expect("open")
        .get("token");
    assert_eq!(
        refusal(read_in(&mut conn, &token).await),
        "read_page_requires_clean_transaction"
    );
    sqlx::query("ROLLBACK")
        .execute(&mut conn)
        .await
        .expect("rollback");
    assert_eq!(refusal(read_in(&mut conn, &token).await), "unknown_token");

    // SAVEPOINT -> RELEASE, SAVEPOINT still open, ROLLBACK TO.
    for (after_open, name) in [
        ("RELEASE SAVEPOINT s", "release"),
        ("SELECT 1", "open savepoint"),
        ("ROLLBACK TO SAVEPOINT s", "rollback to"),
    ] {
        sqlx::query("BEGIN")
            .execute(&mut conn)
            .await
            .expect("begin");
        sqlx::query("SAVEPOINT s")
            .execute(&mut conn)
            .await
            .expect("savepoint");
        let token: String = sqlx::query(open)
            .fetch_one(&mut conn)
            .await
            .expect("open")
            .get("token");
        sqlx::raw_sql(sqlx::AssertSqlSafe(after_open))
            .execute(&mut conn)
            .await
            .expect(name);
        assert_eq!(
            refusal(read_in(&mut conn, &token).await),
            "read_page_requires_clean_transaction",
            "{name}"
        );
        sqlx::query("ROLLBACK")
            .execute(&mut conn)
            .await
            .expect("rollback");
    }

    // DO + EXCEPTION: the intent is rolled back with the subtransaction and
    // the transaction already holds an xid.
    sqlx::query("BEGIN")
        .execute(&mut conn)
        .await
        .expect("begin");
    sqlx::raw_sql(
        "DO $$ DECLARE t text; BEGIN \
           BEGIN \
             SELECT token INTO t FROM audit_store.open_access('export', '{}', 10, 2); \
             RAISE EXCEPTION 'abort'; \
           EXCEPTION WHEN others THEN \
             PERFORM set_config('test.token', t, false); \
           END; \
         END $$",
    )
    .execute(&mut conn)
    .await
    .expect("do block");
    let token: String = sqlx::query("SELECT current_setting('test.token') AS t")
        .fetch_one(&mut conn)
        .await
        .expect("token")
        .get("t");
    assert_eq!(
        refusal(read_in(&mut conn, &token).await),
        "read_page_requires_clean_transaction"
    );
    sqlx::query("COMMIT")
        .execute(&mut conn)
        .await
        .expect("commit");
    assert_eq!(refusal(read_in(&mut conn, &token).await), "unknown_token");

    // A committed intent survives a later transaction that wrote and rolled
    // back, and is then readable in a READ ONLY transaction.
    let token = open_export(&cast).await;
    sqlx::query("BEGIN")
        .execute(&mut conn)
        .await
        .expect("begin");
    sqlx::query("SELECT pg_current_xact_id()")
        .execute(&mut conn)
        .await
        .expect("xid");
    assert_eq!(
        refusal(read_in(&mut conn, &token).await),
        "read_page_requires_clean_transaction"
    );
    sqlx::query("ROLLBACK")
        .execute(&mut conn)
        .await
        .expect("rollback");
    sqlx::query("BEGIN READ ONLY")
        .execute(&mut conn)
        .await
        .expect("begin");
    assert_eq!(read_in(&mut conn, &token).await.expect("readable"), 5);
    sqlx::query("COMMIT")
        .execute(&mut conn)
        .await
        .expect("commit");

    // Another session user cannot use the token.
    let reader2 = db.login("reader2", &["audit_store_reader"]).await;
    let mut other = PgConnection::connect(&reader2.url).await.expect("connect");
    assert_eq!(
        refusal(read_in(&mut other, &token).await),
        "token_session_mismatch"
    );

    // An expired token is refused.
    db.exec(&format!(
        "SET session_replication_role = replica; \
         UPDATE audit_store.access_intents SET expires_at = now() - interval '1 minute' \
         WHERE token_digest = sha256(decode('{token}', 'hex')); \
         RESET session_replication_role;"
    ))
    .await;
    assert_eq!(refusal(read_in(&mut conn, &token).await), "token_expired");
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn disclosure_is_bounded_whatever_after_seq_is() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let ids = ingest_documents(&db, &cast, 25).await;
    let reader = cast.reader.admin().await;
    let token = reader
        .open_access(AccessOperation::Export, &json!({}), 5, 2)
        .await
        .expect("opened");
    assert!(!token.include_control);
    let relay_seqs: Vec<i64> =
        sqlx::query("SELECT seq FROM audit_store.events WHERE origin = 'relay' ORDER BY seq")
            .fetch_all(&db.admin)
            .await
            .expect("seqs")
            .iter()
            .map(|r| r.get("seq"))
            .collect();
    let bound: Vec<i64> = relay_seqs[..10].to_vec();
    // Events stored after the intent are beyond the watermark.
    ingest_documents(&db, &cast, 3).await;
    let mut seen = BTreeSet::new();
    for after in [
        i64::MIN,
        -5,
        0,
        1,
        bound[2],
        bound[7],
        bound[9],
        relay_seqs[20],
        i64::MAX,
    ] {
        let page = reader.read_page(token.secret(), after).await.expect("page");
        assert!(page.len() <= 5);
        for line in &page {
            assert!(
                bound.contains(&line.seq),
                "after {after}: seq {} beyond bound",
                line.seq
            );
            assert!(line.seq > after);
            seen.insert(line.seq);
        }
    }
    assert_eq!(seen.into_iter().collect::<Vec<_>>(), bound);
    let pages = reader.read_all(&token).await.expect("all pages");
    assert_eq!(pages.iter().map(Vec::len).collect::<Vec<_>>(), vec![5, 5]);
    // Filters narrow the bounded set.
    let filtered = reader
        .open_access(
            AccessOperation::Investigate,
            &json!({"event_ids": [ids[0].to_string(), ids[3].to_string()]}),
            10,
            1,
        )
        .await
        .expect("filtered");
    assert_eq!(
        reader
            .read_page(filtered.secret(), 0)
            .await
            .expect("page")
            .len(),
        2
    );
    let typed = reader
        .open_access(
            AccessOperation::Investigate,
            &json!({"event_types": ["document.version.read_confirmed"]}),
            100,
            1,
        )
        .await
        .expect("typed");
    assert_eq!(
        reader
            .read_page(typed.secret(), 0)
            .await
            .expect("page")
            .len(),
        13
    );
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn revocation_and_unbinding_stop_an_open_token() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    ingest_documents(&db, &cast, 3).await;
    let reader = cast.reader.admin().await;
    let token = open_export(&cast).await;
    assert_eq!(
        reader.read_page(&token, 0).await.expect("readable").len(),
        3
    );
    cast.admin
        .admin()
        .await
        .change_access(ISSUER, "reader-1", "export", AccessChange::Revoke)
        .await
        .expect("revoked");
    assert_eq!(
        reader.read_page(&token, 0).await.expect_err("revoked"),
        rejected("access_revoked")
    );
    cast.admin
        .admin()
        .await
        .change_access(ISSUER, "reader-1", "export", AccessChange::Grant)
        .await
        .expect("granted again");
    assert_eq!(
        reader.read_page(&token, 0).await.expect("readable").len(),
        3
    );
    cast.dba
        .owner()
        .await
        .unbind_principal(&cast.reader.role)
        .await
        .expect("unbound");
    assert_eq!(
        reader.read_page(&token, 0).await.expect_err("unbound"),
        rejected("access_revoked")
    );
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn tampering_with_a_stored_intent_is_detected() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    ingest_documents(&db, &cast, 4).await;
    let reader = cast.reader.admin().await;
    let token = reader
        .open_access(
            AccessOperation::Investigate,
            &json!({"event_types": ["document.created"]}),
            10,
            1,
        )
        .await
        .expect("opened");
    assert_eq!(
        reader
            .read_page(token.secret(), 0)
            .await
            .expect("page")
            .len(),
        2
    );
    // Widen the filter and recompute its digest consistently: the chained
    // intent event still carries the original digest.
    db.exec(&format!(
        "SET session_replication_role = replica; \
         UPDATE audit_store.access_intents SET filter = '{{}}'::jsonb, \
                filter_digest = sha256(convert_to('{{}}', 'UTF8')) \
         WHERE intent_seq = {}; \
         RESET session_replication_role;",
        token.intent_seq
    ))
    .await;
    assert_eq!(
        reader
            .read_page(token.secret(), 0)
            .await
            .expect_err("tampered"),
        rejected("intent_integrity_violation")
    );
    // Widening the page bound is detected too.
    let other = reader
        .open_access(AccessOperation::Investigate, &json!({}), 1, 1)
        .await
        .expect("opened");
    db.exec(&format!(
        "SET session_replication_role = replica; \
         UPDATE audit_store.access_intents SET page_size = 100 WHERE intent_seq = {}; \
         RESET session_replication_role;",
        other.intent_seq
    ))
    .await;
    assert_eq!(
        reader
            .read_page(other.secret(), 0)
            .await
            .expect_err("tampered"),
        rejected("intent_integrity_violation")
    );
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn control_events_are_visible_only_with_administer_and_close_is_recorded() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    ingest_documents(&db, &cast, 2).await;
    cast.admin2
        .admin()
        .await
        .change_access(ISSUER, "admin-1", "investigate", AccessChange::Grant)
        .await
        .expect("grant");
    let reader = cast.reader.admin().await;
    let plain = reader
        .open_access(AccessOperation::Investigate, &json!({}), 100, 1)
        .await
        .expect("reader");
    assert!(!plain.include_control);
    let lines = reader.read_page(plain.secret(), 0).await.expect("page");
    assert_eq!(lines.len(), 2);
    assert!(
        lines
            .iter()
            .all(|l| l.line.contains("\"origin\":\"relay\""))
    );

    let admin = cast.admin.admin().await;
    let privileged = admin
        .open_access(AccessOperation::Investigate, &json!({}), 100, 1)
        .await
        .expect("administrator");
    assert!(privileged.include_control);
    let lines = admin.read_page(privileged.secret(), 0).await.expect("page");
    assert!(
        lines
            .iter()
            .any(|l| l.line.contains("\"origin\":\"store\""))
    );
    assert_eq!(
        i64::try_from(lines.len()).expect("len"),
        privileged.watermark,
        "every row up to the watermark"
    );
    let digest = audit_store_postgres::files::page_digest(&lines);
    let closed = admin
        .close_access(
            privileged.secret(),
            i64::try_from(lines.len()).expect("len"),
            std::slice::from_ref(&digest),
        )
        .await
        .expect("closed");
    let close = control_events(&db.admin, "audit.access.closed").await;
    assert_eq!(close.last().expect("closed").0, closed.seq.expect("seq"));
    assert_eq!(close[0].1["intent_seq"], json!(privileged.intent_seq));
    let intent = control_events(&db.admin, "audit.access.intent_opened").await;
    assert_eq!(
        intent.last().expect("intent").1["include_control"],
        json!(true)
    );
    // A token of another session cannot be closed.
    assert_eq!(
        reader
            .close_access(privileged.secret(), 0, &[])
            .await
            .expect_err("not the reader's intent"),
        denied("invalid_input")
    );
    db.assert_store_conforms().await;
}

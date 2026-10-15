//! Closed, failure-only metadata; no resource or error text is accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Load,
    Policy,
    Acknowledgements,
    Agent,
    Evidence,
    Authority,
    Freshness,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Dependency {
    Work,
    Agent,
    Document,
    None,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SqlClass {
    None,
    Connection,
    AcquireTimeout,
    PoolClosed,
    DatabaseConnection,
    TransactionRollback,
    DataException,
    Constraint,
    Resource,
    OtherDatabase,
    Decode,
    Other,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Failure {
    DependencyUnavailable,
    CommitUnknown,
    ArtifactUnavailable,
    Forbidden,
    NotFound,
    Conflict,
    Validation,
    Integrity,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Diagnostic {
    pub phase: Phase,
    pub dependency: Dependency,
    pub sql: SqlClass,
}
impl Default for Diagnostic {
    fn default() -> Self {
        Self {
            phase: Phase::Load,
            dependency: Dependency::Work,
            sql: SqlClass::None,
        }
    }
}
impl SqlClass {
    pub fn database(code: Option<&str>) -> Self {
        match code.and_then(|code| code.get(..2)) {
            Some("08") => Self::DatabaseConnection,
            Some("40") => Self::TransactionRollback,
            Some("22") => Self::DataException,
            Some("23") => Self::Constraint,
            Some("53") => Self::Resource,
            _ => Self::OtherDatabase,
        }
    }
}
impl Diagnostic {
    pub fn json(self, list: bool, failure: Failure, elapsed_ms: u128) -> String {
        let phase = match self.phase {
            Phase::Load => "load",
            Phase::Policy => "policy",
            Phase::Acknowledgements => "acknowledgements",
            Phase::Agent => "agent",
            Phase::Evidence => "evidence",
            Phase::Authority => "authority",
            Phase::Freshness => "freshness",
        };
        let dependency = match self.dependency {
            Dependency::Work => "work",
            Dependency::Agent => "agent",
            Dependency::Document => "document",
            Dependency::None => "none",
        };
        let sql = match self.sql {
            SqlClass::None => "none",
            SqlClass::Connection => "connection",
            SqlClass::AcquireTimeout => "acquire_timeout",
            SqlClass::PoolClosed => "pool_closed",
            SqlClass::DatabaseConnection => "database_connection",
            SqlClass::TransactionRollback => "transaction_rollback",
            SqlClass::DataException => "data_exception",
            SqlClass::Constraint => "constraint",
            SqlClass::Resource => "resource",
            SqlClass::OtherDatabase => "other_database",
            SqlClass::Decode => "decode",
            SqlClass::Other => "other",
        };
        let failure = match failure {
            Failure::DependencyUnavailable => "dependency_unavailable",
            Failure::CommitUnknown => "commit_unknown",
            Failure::ArtifactUnavailable => "artifact_unavailable",
            Failure::Forbidden => "forbidden",
            Failure::NotFound => "not_found",
            Failure::Conflict => "conflict",
            Failure::Validation => "validation",
            Failure::Integrity => "integrity",
        };
        format!(
            "{{\"operation\":\"{}\",\"phase\":\"{phase}\",\"dependency\":\"{dependency}\",\"sql_class\":\"{sql}\",\"failure\":\"{failure}\",\"elapsed_ms\":{}}}",
            if list { "finding_list" } else { "finding" },
            elapsed_ms.min(u32::MAX as u128)
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sqlstate_is_projected_without_unknown_text() {
        for (code, expected) in [
            (Some("08006"), SqlClass::DatabaseConnection),
            (Some("40001"), SqlClass::TransactionRollback),
            (Some("22000"), SqlClass::DataException),
            (Some("23505"), SqlClass::Constraint),
            (Some("53100"), SqlClass::Resource),
            (Some("https://secret.invalid/id"), SqlClass::OtherDatabase),
            (Some("é"), SqlClass::OtherDatabase),
            (None, SqlClass::OtherDatabase),
        ] {
            assert_eq!(SqlClass::database(code), expected);
        }
    }
    #[test]
    fn emits_only_closed_bounded_fields() {
        let d = Diagnostic {
            phase: Phase::Evidence,
            dependency: Dependency::Document,
            sql: SqlClass::None,
        };
        assert_eq!(
            d.json(false, Failure::DependencyUnavailable, 5123),
            r#"{"operation":"finding","phase":"evidence","dependency":"document","sql_class":"none","failure":"dependency_unavailable","elapsed_ms":5123}"#
        );
        assert!(d.json(true, Failure::Integrity, u128::MAX).len() < 256);
        assert!(
            d.json(true, Failure::Integrity, u128::MAX)
                .contains("\"elapsed_ms\":4294967295")
        );
    }
}

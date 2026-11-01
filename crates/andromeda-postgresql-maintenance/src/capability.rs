//! Per-database validation of the extension and privileges maintenance needs.
//!
//! Inspection and acting are checked apart. The `pgstattuple` extension and
//! its functions gate a whole visit: without them nothing can be sampled. The
//! privilege to maintain a branch's heaps is checked per branch and only
//! decides whether automatic mode acts there; a branch without it is still
//! sampled.
//!
//! PostgreSQL 17 grants maintenance through `MAINTAIN`, which owners and
//! superusers hold implicitly. An earlier server has no such privilege: there
//! only an owner or a superuser may vacuum, analyze and reindex a table, so
//! ownership is what the check asks for, and every server answers the same
//! question.

use crate::{branch::Branch, error::MaintenanceError};
use andromeda_postgresql_context_store::schema_contract::server_requirements::PostgresVersion;
use andromeda_postgresql_seaquery::{
    introspection::server_version_num,
    maintenance::{current_user_has_function_privilege, current_user_has_table_privilege, current_user_owns_relation, to_regprocedure},
    statement::Statement,
};
use sea_query::{Alias, Expr, ExprTrait, Query, SelectStatement};
use sqlx::PgConnection;

/// Whether a tenant database can be inspected at all.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CapabilityStatus {
    /// The extension is installed and the role may execute its functions.
    Ready,
    /// The optional supplied extension is not installed.
    MissingPgstattuple,
    /// The role cannot execute required physical inspection functions.
    MissingPgstattupleExecute,
}

/// Whether the broker role may run maintenance statements in one branch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MaintainPrivilege {
    /// The role may maintain every allowlisted heap of the branch.
    Held,
    /// The role may not maintain at least one allowlisted heap of the branch:
    /// it lacks `MAINTAIN` on PostgreSQL 17 and later, ownership before.
    NotGranted,
}

/// Validate the optional diagnostic extension and the role's privilege to run
/// its inspection functions.
pub(crate) async fn validate_capabilities(connection: &mut PgConnection) -> Result<CapabilityStatus, MaintenanceError> {
    if !extension_present_statement().to_query_scalar::<bool>().fetch_one(&mut *connection).await? {
        return Ok(CapabilityStatus::MissingPgstattuple);
    }
    if !inspection_privilege_statement().to_query_scalar::<bool>().fetch_one(&mut *connection).await? {
        return Ok(CapabilityStatus::MissingPgstattupleExecute);
    }
    Ok(CapabilityStatus::Ready)
}

/// The first PostgreSQL release with the `MAINTAIN` privilege.
const MAINTAIN_PRIVILEGE_VERSION: PostgresVersion = PostgresVersion::new(170_000);

/// Whether the role may maintain each of `branches`, in order.
pub(crate) async fn maintain_privileges(connection: &mut PgConnection, branches: &[Branch]) -> Result<Vec<(Branch, MaintainPrivilege)>, MaintenanceError> {
    let server = PostgresVersion::new(server_version_statement().to_query_scalar::<i32>().fetch_one(&mut *connection).await?);
    let mut privileges = Vec::with_capacity(branches.len());
    for &branch in branches {
        let privilege = if maintain_probe(server, branch).to_query_scalar::<bool>().fetch_one(&mut *connection).await? {
            MaintainPrivilege::Held
        } else {
            MaintainPrivilege::NotGranted
        };
        privileges.push((branch, privilege));
    }
    Ok(privileges)
}

/// The running server's `server_version_num`.
fn server_version_statement() -> SelectStatement {
    Query::select().expr(server_version_num()).to_owned()
}

/// The query whether the role may maintain `branch` on a server at version
/// `server`: `MAINTAIN` from PostgreSQL 17 on, ownership before, where the
/// privilege name is rejected.
fn maintain_probe(server: PostgresVersion, branch: Branch) -> SelectStatement {
    if server >= MAINTAIN_PRIVILEGE_VERSION {
        maintain_privilege_statement(branch)
    } else {
        ownership_statement(branch)
    }
}

/// Ownership, direct or inherited, of every allowlisted heap `branch` holds,
/// which a superuser always passes.
fn ownership_statement(branch: Branch) -> SelectStatement {
    let owned = branch
        .heaps()
        .map(|heap| current_user_owns_relation(Expr::val(branch.qualified(heap.relation_name()))))
        .fold(Expr::val(true), ExprTrait::and);
    Query::select().expr(owned).to_owned()
}

/// `pg_extension` presence query.
fn extension_present_statement() -> SelectStatement {
    let inner = Query::select()
        .expr(Expr::val(1))
        .from(Alias::new("pg_extension"))
        .and_where(Expr::col(Alias::new("extname")).eq("pgstattuple"))
        .to_owned();
    Query::select().expr(Expr::exists(inner)).to_owned()
}

/// Required `pgstattuple` function execution privileges.
fn inspection_privilege_statement() -> SelectStatement {
    let exact = current_user_has_function_privilege(to_regprocedure(Expr::val("pgstattuple(regclass)")), Expr::val("EXECUTE"));
    let approximate = current_user_has_function_privilege(to_regprocedure(Expr::val("pgstattuple_approx(regclass)")), Expr::val("EXECUTE"));
    Query::select().expr(exact.and(approximate)).to_owned()
}

/// Required PostgreSQL 17 `MAINTAIN` privileges on every allowlisted heap
/// `branch` holds; an index is rebuilt under its heap's privilege.
fn maintain_privilege_statement(branch: Branch) -> SelectStatement {
    let granted = branch
        .heaps()
        .map(|heap| current_user_has_table_privilege(Expr::val(branch.qualified(heap.relation_name())), Expr::val("MAINTAIN")))
        .fold(Expr::val(true), ExprTrait::and);
    Query::select().expr(granted).to_owned()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        branch::Branch,
        candidate::heap_target::HeapTarget,
        capability::{maintain_privilege_statement, maintain_probe, ownership_statement, server_version_statement},
    };
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use andromeda_postgresql_context_store::schema_contract::server_requirements::PostgresVersion;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn automatic_mode_requires_maintain_on_every_allowlisted_heap() {
        let sql = maintain_privilege_statement(Branch::Live).to_string(PostgresQueryBuilder);
        for heap in HeapTarget::ALL {
            let privilege = format!("has_table_privilege(current_user, 'public.{}', 'MAINTAIN')", heap.relation_name());
            assert!(sql.contains(&privilege), "{heap}: {sql}");
        }
    }

    #[test]
    fn a_snapshot_branch_requires_maintain_on_its_branch_heaps_only() {
        let branch = Branch::Snapshot(SnapshotKey::try_from(7).expect("positive snapshot key"));
        let sql = maintain_privilege_statement(branch).to_string(PostgresQueryBuilder);
        for heap in HeapTarget::ALL {
            let privilege = format!("has_table_privilege(current_user, 'snap_7.{}', 'MAINTAIN')", heap.relation_name());
            assert_eq!(sql.contains(&privilege), branch.heaps().any(|held| held == heap), "{heap}: {sql}");
        }
        assert!(!sql.contains("'public."), "{sql}");
    }

    /// The privilege arrived in PostgreSQL 17; an earlier server rejects its
    /// name, and only an owner or a superuser may maintain a table there.
    #[test]
    fn a_server_before_postgresql_17_asks_for_ownership_instead_of_maintain() {
        let expected = ownership_statement(Branch::Live).to_string(PostgresQueryBuilder);
        for version in [150_000, 160_000, 169_999] {
            let sql = maintain_probe(PostgresVersion::new(version), Branch::Live).to_string(PostgresQueryBuilder);
            assert_eq!(sql, expected, "{version}");
            assert!(!sql.contains("MAINTAIN"), "{version}: {sql}");
        }
    }

    #[test]
    fn ownership_is_required_on_every_allowlisted_heap_of_the_branch() {
        let branch = Branch::Snapshot(SnapshotKey::try_from(7).expect("positive snapshot key"));
        let sql = ownership_statement(branch).to_string(PostgresQueryBuilder);
        for heap in HeapTarget::ALL {
            let owned = format!("'snap_7.{}'::regclass", heap.relation_name());
            assert_eq!(sql.contains(&owned), branch.heaps().any(|held| held == heap), "{heap}: {sql}");
        }
    }

    #[test]
    fn a_server_from_postgresql_17_on_builds_the_privilege_query() {
        let expected = maintain_privilege_statement(Branch::Live).to_string(PostgresQueryBuilder);
        for version in [170_000, 180_001] {
            assert_eq!(
                maintain_probe(PostgresVersion::new(version), Branch::Live).to_string(PostgresQueryBuilder),
                expected,
                "{version}"
            );
        }
    }

    #[test]
    fn the_server_version_is_read_as_an_integer() {
        assert_eq!(
            server_version_statement().to_string(PostgresQueryBuilder),
            "SELECT current_setting('server_version_num')::integer"
        );
    }
}

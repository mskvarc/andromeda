//! Per-database validation of the extension and privileges maintenance needs.

use crate::{candidate::HeapTarget, error::MaintenanceError, scalar_query::select_bool};
use andromeda_postgresql_seaquery::maintenance::{current_user_has_function_privilege, current_user_has_table_privilege, to_regprocedure};
use sea_query::{Alias, Expr, ExprTrait, Query, SelectStatement};
use sqlx::PgConnection;

/// Per-database capability validation result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CapabilityStatus {
    /// Required extension, function execution and optional mutation privileges exist.
    Ready,
    /// The optional supplied extension is not installed.
    MissingPgstattuple,
    /// The role cannot execute required physical inspection functions.
    MissingPgstattupleExecute,
    /// Automatic mode lacks `MAINTAIN` on at least one allowlisted heap.
    MissingMaintain,
}

/// Whether capability validation includes mutation privileges.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MutationPrivilege {
    /// Inspection privileges only.
    NotRequired,
    /// PostgreSQL 18 `MAINTAIN` is required on all allowlisted heaps.
    Required,
}

/// Validate the optional diagnostic extension and role privileges.
pub(crate) async fn validate_capabilities(connection: &mut PgConnection, mutation: MutationPrivilege) -> Result<CapabilityStatus, MaintenanceError> {
    if !select_bool(connection, extension_present_statement()).await? {
        return Ok(CapabilityStatus::MissingPgstattuple);
    }
    if !select_bool(connection, inspection_privilege_statement()).await? {
        return Ok(CapabilityStatus::MissingPgstattupleExecute);
    }
    if mutation == MutationPrivilege::Required && !select_bool(connection, maintain_privilege_statement()).await? {
        return Ok(CapabilityStatus::MissingMaintain);
    }
    Ok(CapabilityStatus::Ready)
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

/// Required PostgreSQL 18 `MAINTAIN` privileges on every allowlisted heap;
/// an index is rebuilt under its heap's privilege.
fn maintain_privilege_statement() -> SelectStatement {
    let granted = HeapTarget::ALL
        .map(|heap| current_user_has_table_privilege(Expr::val(format!("public.{}", heap.relation_name())), Expr::val("MAINTAIN")))
        .into_iter()
        .fold(Expr::val(true), ExprTrait::and);
    Query::select().expr(granted).to_owned()
}

#[cfg(test)]
mod tests {
    use crate::{candidate::HeapTarget, capability::maintain_privilege_statement};
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn automatic_mode_requires_maintain_on_every_allowlisted_heap() {
        let sql = maintain_privilege_statement().to_string(PostgresQueryBuilder);
        for heap in HeapTarget::ALL {
            let privilege = format!("has_table_privilege(current_user, 'public.{}', 'MAINTAIN')", heap.relation_name());
            assert!(sql.contains(&privilege), "{heap}: {sql}");
        }
    }
}

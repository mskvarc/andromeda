//! Per-database validation of the extension and privileges maintenance needs.
//!
//! The `pgstattuple` extension and its functions measure two things: the
//! exact free space of a probeable index, and the estimated free space of a
//! heap a repack would give back. Without it, or without the privilege to
//! execute both, a visit probes no index and no heap and runs every other
//! check, all of which read the statistics views and the catalogs. The
//! `pg_repack` extension, on the release line the controller drives, and the
//! rights to run it decide whether a repack may run; without them repacks are
//! only reported. The privilege to maintain
//! a branch's heaps is checked per branch and only decides whether automatic
//! mode acts there; a branch without it is still sampled. The right one
//! action needs on the relation it works on, beyond the heaps it vacuums, is
//! checked on that relation alone.
//!
//! PostgreSQL 17 grants maintenance through `MAINTAIN`, which owners and
//! superusers hold implicitly. An earlier server has no such privilege: there
//! only an owner or a superuser may vacuum, analyze and reindex a table, so
//! ownership is what the check asks for, and every server answers the same
//! question.

use crate::{
    branch::Branch,
    error::MaintenanceError,
    repack::version::{RepackSupport, RepackVersion},
};
use andromeda_postgresql_codes::maintenance_inspection_result::MaintenanceInspectionResultCode;
use andromeda_postgresql_context_store::schema_contract::server_requirements::PostgresVersion;
use andromeda_postgresql_seaquery::{
    introspection::{current_role_is_superuser, server_version_num},
    maintenance::{
        current_user_has_function_privilege,
        current_user_has_schema_privilege,
        current_user_has_table_privilege,
        current_user_owns_relation,
        to_regprocedure,
    },
    repack::repack_version,
    statement::Statement,
};
use sea_query::{Alias, Expr, ExprTrait, Query, SelectStatement, SimpleExpr};
use sqlx::PgConnection;
use strum::IntoStaticStr;

/// Whether a visit may measure an index with `pgstattuple`.
#[derive(Clone, Copy, Debug, Eq, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum ProbeCapability {
    /// The extension is installed and the role may execute `pgstattuple`.
    Ready,
    /// The extension is not installed in the tenant database.
    MissingPgstattuple,
    /// The extension is installed and the role may not execute `pgstattuple`
    /// or `pgstattuple_approx`.
    MissingPgstattupleExecute,
}

/// Whether a repack may run in a tenant database.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RepackCapability {
    /// The extension runs a supported version and the role may run it.
    Ready(RepackVersion),
    /// The extension is not installed in the tenant database.
    MissingExtension,
    /// The extension runs a version the controller does not drive.
    UnsupportedVersion(RepackVersion),
    /// The role is no superuser and may not execute the extension's
    /// functions or create its objects in the `repack` schema.
    MissingRight,
}

impl RepackCapability {
    /// The missing capability's name, as the inspection result registry
    /// spells it, `None` when nothing is missing.
    pub(crate) const fn missing(self) -> Option<&'static str> {
        match self {
            RepackCapability::Ready(_) => None,
            RepackCapability::MissingExtension => Some(MaintenanceInspectionResultCode::MissingPgRepack.label()),
            RepackCapability::UnsupportedVersion(_) => Some(MaintenanceInspectionResultCode::UnsupportedPgRepack.label()),
            RepackCapability::MissingRight => Some(MaintenanceInspectionResultCode::MissingRepackRight.label()),
        }
    }
}

/// The `repack` functions a run calls, by signature.
const REPACK_FUNCTIONS: [&str; 11] = [
    "repack.version()",
    "repack.get_order_by(oid,oid)",
    "repack.repack_indexdef(oid,oid,name,boolean)",
    "repack.conflicted_triggers(oid)",
    "repack.create_index_type(oid,oid)",
    "repack.create_log_table(oid)",
    "repack.create_table(oid,name)",
    "repack.disable_autovacuum(regclass)",
    "repack.repack_apply(cstring,cstring,cstring,cstring,cstring,integer)",
    "repack.repack_swap(oid)",
    "repack.repack_drop(oid,integer)",
];

/// Whether a repack may run in the tenant database: the extension, its
/// version, and the role's rights beyond the ownership of the table each
/// run checks on its own table.
pub(crate) async fn repack_capability(connection: &mut PgConnection) -> Result<RepackCapability, MaintenanceError> {
    if !extension_present_statement("pg_repack")
        .to_query_scalar::<bool>()
        .fetch_one(&mut *connection)
        .await?
    {
        return Ok(RepackCapability::MissingExtension);
    }
    let version = RepackVersion::try_from(repack_version().to_query_scalar::<String>().fetch_one(&mut *connection).await?.as_str())?;
    if version.support() == RepackSupport::Unsupported {
        return Ok(RepackCapability::UnsupportedVersion(version));
    }
    Ok(if repack_right_statement().to_query_scalar::<bool>().fetch_one(connection).await? {
        RepackCapability::Ready(version)
    } else {
        RepackCapability::MissingRight
    })
}

impl ProbeCapability {
    /// The probes a visit runs out of `candidates`: all of them when the
    /// extension can measure them, none otherwise.
    pub(crate) fn probe_candidates<T>(self, candidates: Vec<T>) -> Vec<T> {
        match self {
            ProbeCapability::Ready => candidates,
            ProbeCapability::MissingPgstattuple | ProbeCapability::MissingPgstattupleExecute => Vec::new(),
        }
    }
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

/// The right a statement needs on the relation it works on, named in a log
/// after its absence.
#[derive(Clone, Copy, Debug, Eq, IntoStaticStr, PartialEq)]
pub(crate) enum MaintainRight {
    /// Ownership of the relation, direct or inherited.
    #[strum(serialize = "missing-ownership")]
    Ownership,
    /// The PostgreSQL 17 `MAINTAIN` privilege on the relation.
    #[strum(serialize = "missing-maintain")]
    Maintain,
}

/// Whether the optional `pgstattuple` extension is installed and the role may
/// execute the one function an index probe calls.
pub(crate) async fn probe_capability(connection: &mut PgConnection) -> Result<ProbeCapability, MaintenanceError> {
    if !extension_present_statement("pgstattuple")
        .to_query_scalar::<bool>()
        .fetch_one(&mut *connection)
        .await?
    {
        return Ok(ProbeCapability::MissingPgstattuple);
    }
    if !probe_privilege_statement().to_query_scalar::<bool>().fetch_one(&mut *connection).await? {
        return Ok(ProbeCapability::MissingPgstattupleExecute);
    }
    Ok(ProbeCapability::Ready)
}

/// The first PostgreSQL release with the `MAINTAIN` privilege.
const MAINTAIN_PRIVILEGE_VERSION: PostgresVersion = PostgresVersion::new(170_000);

/// Whether the role may maintain each of `branches`, in order.
pub(crate) async fn maintain_privileges(connection: &mut PgConnection, branches: &[Branch]) -> Result<Vec<(Branch, MaintainPrivilege)>, MaintenanceError> {
    let server = server_version(connection).await?;
    let mut privileges = Vec::with_capacity(branches.len());
    for &branch in branches {
        privileges.push((branch, privilege_of(connection, &maintain_probe(server, branch)).await?));
    }
    Ok(privileges)
}

/// The running server's version.
pub(crate) async fn server_version(connection: &mut PgConnection) -> Result<PostgresVersion, MaintenanceError> {
    Ok(PostgresVersion::new(
        server_version_statement().to_query_scalar::<i32>().fetch_one(connection).await?,
    ))
}

/// Whether the role owns `relation`, directly or through a role it inherits.
pub(crate) async fn ownership(connection: &mut PgConnection, relation: &str) -> Result<MaintainPrivilege, MaintenanceError> {
    privilege_of(connection, &relation_ownership_statement(relation)).await
}

/// Whether the role may maintain `relation` on a server at version `server`.
pub(crate) async fn relation_maintain(connection: &mut PgConnection, server: PostgresVersion, relation: &str) -> Result<MaintainPrivilege, MaintenanceError> {
    privilege_of(connection, &relation_maintain_probe(server, relation)).await
}

/// The right `relation_maintain` asks for on a server at version `server`.
pub(crate) fn maintain_right(server: PostgresVersion) -> MaintainRight {
    if server >= MAINTAIN_PRIVILEGE_VERSION {
        MaintainRight::Maintain
    } else {
        MaintainRight::Ownership
    }
}

/// Run a privilege query answering one boolean.
async fn privilege_of(connection: &mut PgConnection, statement: &SelectStatement) -> Result<MaintainPrivilege, MaintenanceError> {
    Ok(if statement.to_query_scalar::<bool>().fetch_one(connection).await? {
        MaintainPrivilege::Held
    } else {
        MaintainPrivilege::NotGranted
    })
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

/// Ownership, direct or inherited, of `relation`, which a superuser always
/// passes.
fn relation_ownership_statement(relation: &str) -> SelectStatement {
    Query::select().expr(current_user_owns_relation(Expr::val(relation))).to_owned()
}

/// The query whether the role may maintain `relation` on a server at version
/// `server`: `MAINTAIN` from PostgreSQL 17 on, ownership before.
fn relation_maintain_probe(server: PostgresVersion, relation: &str) -> SelectStatement {
    match maintain_right(server) {
        MaintainRight::Maintain => Query::select()
            .expr(current_user_has_table_privilege(Expr::val(relation), Expr::val("MAINTAIN")))
            .to_owned(),
        MaintainRight::Ownership => relation_ownership_statement(relation),
    }
}

/// Whether the extension `name` is installed in the database.
fn extension_present_statement(name: &str) -> SelectStatement {
    let inner = Query::select()
        .expr(Expr::val(1))
        .from(Alias::new("pg_extension"))
        .and_where(Expr::col(Alias::new("extname")).eq(name))
        .to_owned();
    Query::select().expr(Expr::exists(inner)).to_owned()
}

/// The functions a probe calls: `pgstattuple(regclass)` measures an index,
/// `pgstattuple_approx(regclass)` a heap and its TOAST table.
const PROBE_FUNCTIONS: [&str; 2] = ["pgstattuple(regclass)", "pgstattuple_approx(regclass)"];

/// The privilege to execute every function a probe calls.
fn probe_privilege_statement() -> SelectStatement {
    Query::select().expr(executes_every(&PROBE_FUNCTIONS)).to_owned()
}

/// Whether the role may execute every function of `signatures`.
fn executes_every(signatures: &[&str]) -> SimpleExpr {
    signatures
        .iter()
        .map(|&signature| current_user_has_function_privilege(to_regprocedure(Expr::val(signature)), Expr::val("EXECUTE")))
        .reduce(ExprTrait::and)
        .unwrap_or_else(|| Expr::val(true))
}

/// Whether the role may run a repack beyond owning its table: as a
/// superuser, or with `EXECUTE` on every function a run calls, `SELECT` on
/// the `repack.tables` view its statements are read from, which the
/// extension grants nobody, and `USAGE` and `CREATE` on the `repack` schema,
/// where a run creates its log and copy tables.
fn repack_right_statement() -> SelectStatement {
    let schema = |privilege: &'static str| current_user_has_schema_privilege(Expr::val("repack"), Expr::val(privilege));
    let script = current_user_has_table_privilege(Expr::val("repack.tables"), Expr::val("SELECT"));
    let granted = executes_every(&REPACK_FUNCTIONS).and(script).and(schema("USAGE")).and(schema("CREATE"));
    Query::select().expr(current_role_is_superuser().or(granted)).to_owned()
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
mod tests {
    use crate::{
        branch::Branch,
        candidate::{heap_target::HeapTarget, index_target::IndexTarget},
        capability::{
            MaintainRight,
            ProbeCapability,
            RepackCapability,
            maintain_privilege_statement,
            maintain_probe,
            maintain_right,
            ownership_statement,
            probe_privilege_statement,
            relation_maintain_probe,
            relation_ownership_statement,
            repack_right_statement,
            server_version_statement,
        },
        inspection::{IndexSize, pgstattuple_statement},
        repack::version::{RepackVersion, RepackVersionError},
    };
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use andromeda_postgresql_context_store::schema_contract::server_requirements::PostgresVersion;
    use andromeda_postgresql_seaquery::repack_catalog::approximate_free_space;
    use pretty_assertions::assert_eq;
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

    fn candidates() -> Vec<IndexSize> {
        [IndexTarget::PkCaGrain, IndexTarget::IdxCaValueGeometry]
            .map(|target| IndexSize {
                branch: Branch::Live,
                target,
                bytes: 1 << 30,
            })
            .to_vec()
    }

    #[test]
    fn a_visit_with_pgstattuple_runs_every_probe_candidate() {
        assert_eq!(ProbeCapability::Ready.probe_candidates(candidates()), candidates());
    }

    #[test]
    fn a_visit_without_pgstattuple_or_its_execute_privilege_probes_no_index() {
        for capability in [ProbeCapability::MissingPgstattuple, ProbeCapability::MissingPgstattupleExecute] {
            assert_eq!(capability.probe_candidates(candidates()), [], "{capability:?}");
        }
    }

    #[test]
    fn the_probe_privilege_check_asks_only_for_the_functions_a_probe_calls() {
        assert_eq!(
            probe_privilege_statement().to_string(PostgresQueryBuilder),
            "SELECT has_function_privilege(current_user, to_regprocedure('pgstattuple(regclass)'), 'EXECUTE') AND has_function_privilege(current_user, to_regprocedure('pgstattuple_approx(regclass)'), 'EXECUTE')"
        );
    }

    #[test]
    fn every_function_the_probe_privilege_check_names_is_one_a_probe_calls() {
        let privilege = probe_privilege_statement().to_string(PostgresQueryBuilder);
        let probe = pgstattuple_statement("public.pk_ca_grain").to_string(PostgresQueryBuilder);
        let named: Vec<&str> = privilege
            .split("to_regprocedure('")
            .skip(1)
            .filter_map(|rest| rest.split_once("(regclass)')"))
            .map(|(function, _)| function)
            .collect();
        assert_eq!(named, ["pgstattuple", "pgstattuple_approx"]);
        assert!(probe.contains("pgstattuple(to_regclass("), "{probe}");
        assert!(
            approximate_free_space(16_384).to_string(PostgresQueryBuilder).contains("pgstattuple_approx("),
            "a heap probe calls pgstattuple_approx"
        );
    }

    #[test]
    fn a_relation_is_maintained_through_maintain_from_postgresql_17_and_through_ownership_before() {
        let chunk = "_timescaledb_internal._hyper_1_2_chunk";
        assert_eq!(
            relation_maintain_probe(PostgresVersion::new(170_000), chunk).to_string(PostgresQueryBuilder),
            "SELECT has_table_privilege(current_user, '_timescaledb_internal._hyper_1_2_chunk', 'MAINTAIN')"
        );
        assert_eq!(
            relation_maintain_probe(PostgresVersion::new(160_004), chunk).to_string(PostgresQueryBuilder),
            relation_ownership_statement(chunk).to_string(PostgresQueryBuilder)
        );
        assert_eq!(maintain_right(PostgresVersion::new(170_000)), MaintainRight::Maintain);
        assert_eq!(maintain_right(PostgresVersion::new(160_004)), MaintainRight::Ownership);
    }

    #[test]
    fn relation_ownership_counts_membership_in_the_owning_role() {
        assert_eq!(
            relation_ownership_statement("public.attribute_history").to_string(PostgresQueryBuilder),
            r#"SELECT pg_has_role(current_user, (SELECT "relowner" FROM "pg_class" WHERE "oid" = ('public.attribute_history'::regclass)), 'USAGE')"#
        );
    }

    #[test]
    fn a_missing_right_is_named_after_what_is_missing() {
        assert_eq!(<&str>::from(MaintainRight::Ownership), "missing-ownership");
        assert_eq!(<&str>::from(MaintainRight::Maintain), "missing-maintain");
    }

    #[test]
    fn the_missing_capability_is_named_after_what_is_missing() {
        assert_eq!(<&str>::from(ProbeCapability::MissingPgstattuple), "missing-pgstattuple");
        assert_eq!(<&str>::from(ProbeCapability::MissingPgstattupleExecute), "missing-pgstattuple-execute");
    }

    #[test]
    fn a_superuser_or_an_owner_with_execute_and_the_repack_schema_may_run_a_repack() {
        let sql = repack_right_statement().to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"SELECT (SELECT "rolsuper" FROM "pg_roles" WHERE "rolname" = (current_user)) OR "#),
            "{sql}"
        );
        for function in [
            "repack.repack_swap(oid)",
            "repack.repack_drop(oid,integer)",
            "repack.repack_apply(cstring,cstring,cstring,cstring,cstring,integer)",
        ] {
            assert!(sql.contains(&format!("to_regprocedure('{function}'), 'EXECUTE')")), "{function}: {sql}");
        }
        assert!(sql.contains("has_schema_privilege(current_user, 'repack', 'USAGE')"), "{sql}");
        assert!(sql.contains("has_schema_privilege(current_user, 'repack', 'CREATE')"), "{sql}");
        assert!(sql.contains("has_table_privilege(current_user, 'repack.tables', 'SELECT')"), "{sql}");
    }

    #[test]
    fn each_missing_repack_capability_is_named_as_the_registry_names_it() -> Result<(), RepackVersionError> {
        let version = RepackVersion::try_from("pg_repack 1.5.3")?;
        assert_eq!(RepackCapability::Ready(version).missing(), None);
        assert_eq!(RepackCapability::MissingExtension.missing(), Some("missing-pg-repack"));
        assert_eq!(
            RepackCapability::UnsupportedVersion(RepackVersion::try_from("pg_repack 1.4.8")?).missing(),
            Some("unsupported-pg-repack")
        );
        assert_eq!(RepackCapability::MissingRight.missing(), Some("missing-repack-right"));
        Ok(())
    }
}

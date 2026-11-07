//! Cheap catalog sampling and bounded physical probes.

use crate::{
    artifact::RecoveryArtifact,
    branch::Branch,
    candidate::index_target::IndexTarget,
    catalog_counter::{CatalogCounter, non_negative, percent},
    error::MaintenanceError,
    policy::{DegradationSample, PolicyProfile},
};
use andromeda_context_space::snapshot_key::SnapshotKey;
use andromeda_postgresql_branch::{PUBLISHED_SNAPSHOT_SCHEMA_PATTERN, published_snapshot_key};
use andromeda_postgresql_seaquery::{
    command::Command,
    maintenance::{pgstattuple, relation_size, session_settings, to_regclass, total_relation_size},
    statement::Statement,
};
use sea_query::{Alias, Expr, ExprTrait, JoinType, Query, SelectStatement, SimpleExpr, extension::postgres::PgBinOper};
use sqlx::{PgConnection, Row};
use std::str::FromStr;

/// An allowlisted index's cheap catalog size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct IndexSize {
    /// The branch whose schema holds the index.
    pub(crate) branch: Branch,
    /// Compile-time target recovered from the exact catalog name.
    pub(crate) target: IndexTarget,
    /// Current physical bytes.
    pub(crate) bytes: u64,
}

/// An invalid index reported by PostgreSQL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InvalidIndex {
    /// The branch whose schema holds the index.
    branch: Branch,
    /// Catalog name, retained for reporting and validated artifact recovery.
    name: String,
}

impl InvalidIndex {
    /// Return a recovery artifact only when the name belongs to the allowlist.
    pub(crate) fn recovery_artifact(&self) -> Option<RecoveryArtifact> {
        RecoveryArtifact::parse(self.branch, &self.name)
    }

    /// The branch whose schema holds the index.
    pub(crate) const fn branch(&self) -> Branch {
        self.branch
    }

    /// Borrow the catalog name for structured reporting.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}

/// Configure one short-lived maintenance session.
pub(crate) async fn configure_session(connection: &mut PgConnection) -> Result<(), MaintenanceError> {
    for statement in session_settings() {
        statement.to_query().execute(&mut *connection).await?;
    }
    Ok(())
}

/// Inspect the sizes of every allowlisted index of `branch` from cheap
/// catalogs.
pub(crate) async fn index_sizes(connection: &mut PgConnection, branch: Branch) -> Result<Vec<IndexSize>, MaintenanceError> {
    let rows = index_sizes_statement(branch).to_query().fetch_all(connection).await?;
    rows.into_iter()
        .map(|row| {
            let name = row.try_get::<String, _>(CatalogColumn::Indexrelname.as_str())?;
            let target = IndexTarget::from_str(&name).map_err(MaintenanceError::UnknownTarget)?;
            Ok(IndexSize {
                branch,
                target,
                bytes: non_negative(row.try_get(CatalogColumn::Bytes.as_str())?, CatalogCounter::IndexBytes)?,
            })
        })
        .collect()
}

/// Perform the one detailed physical probe allowed in a pass.
pub(crate) async fn probe_index(connection: &mut PgConnection, size: IndexSize, profile: PolicyProfile) -> Result<DegradationSample, MaintenanceError> {
    let qualified = size.branch.qualified(size.target.relation_name());
    let row = pgstattuple_statement(&qualified).to_query().fetch_one(connection).await?;
    let reclaimable_bytes = non_negative(row.try_get(CatalogColumn::FreeSpace.as_str())?, CatalogCounter::IndexFreeBytes)?;
    let physical_bytes = non_negative(row.try_get(CatalogColumn::TableLength.as_str())?, CatalogCounter::IndexPhysicalBytes)?;
    Ok(DegradationSample {
        relation_bytes: size.bytes,
        reclaimable_bytes,
        // `pgstattuple` derives `free_percent` as `100 * free_space / table_len`;
        // deriving it from the two byte counters keeps the sample an exact
        // integer instead of narrowing the extension's own float.
        degraded_percent: percent(reclaimable_bytes, physical_bytes),
        threshold_percent: profile.index_free_percent,
    })
}

/// Read every invalid index of the live branch and of every published
/// Snapshot branch, so unrelated corruption is still visible.
pub(crate) async fn invalid_indexes(connection: &mut PgConnection) -> Result<Vec<InvalidIndex>, MaintenanceError> {
    let rows = invalid_indexes_statement().to_query().fetch_all(connection).await?;
    rows.into_iter()
        .map(|row| {
            let schema = row.try_get::<String, _>(CatalogColumn::Nspname.as_str())?;
            let branch = Branch::from_schema(&schema).ok_or(MaintenanceError::UnknownBranchSchema)?;
            Ok(InvalidIndex {
                branch,
                name: row.try_get(CatalogColumn::Relname.as_str())?,
            })
        })
        .collect()
}

/// Read the Snapshot key of every published branch schema the tenant
/// database holds.
pub(crate) async fn published_branches(connection: &mut PgConnection) -> Result<Vec<SnapshotKey>, MaintenanceError> {
    let schemas = published_branches_statement().to_query_scalar::<String>().fetch_all(connection).await?;
    schemas
        .into_iter()
        .map(|schema| published_snapshot_key(&schema).ok_or(MaintenanceError::UnknownBranchSchema))
        .collect()
}

/// Read an index's current physical bytes in `branch`, compared across its
/// rebuild.
pub(crate) async fn index_bytes(connection: &mut PgConnection, branch: Branch, index: IndexTarget) -> Result<u64, MaintenanceError> {
    let statement = Query::select()
        .expr(total_relation_size(to_regclass(Expr::val(branch.qualified(index.relation_name())))))
        .to_owned();
    non_negative(statement.to_query_scalar::<i64>().fetch_one(connection).await?, CatalogCounter::RelationBytes)
}

/// Cheap catalog size query for every allowlisted index of `branch`, each
/// matched on its own heap.
fn index_sizes_statement(branch: Branch) -> SelectStatement {
    let pairs = IndexTarget::ALL.map(|index| Expr::tuple([Expr::val(index.heap().relation_name()), Expr::val(index.relation_name())]));
    Query::select()
        .column(Alias::new(CatalogColumn::Indexrelname.as_str()))
        .expr_as(relation_size(Expr::col(Alias::new("indexrelid"))), Alias::new(CatalogColumn::Bytes.as_str()))
        .from(Alias::new("pg_stat_user_indexes"))
        .and_where(Expr::col(Alias::new("schemaname")).eq(branch.schema_name()))
        .and_where(Expr::tuple([Expr::col(Alias::new("relname")), Expr::col(Alias::new("indexrelname"))]).is_in(pairs))
        .to_owned()
}

/// Detailed physical tuple query against one bound `regclass`.
pub(crate) fn pgstattuple_statement(relation: &str) -> SelectStatement {
    Query::select()
        .columns([Alias::new(CatalogColumn::FreeSpace.as_str()), Alias::new(CatalogColumn::TableLength.as_str())])
        .from_function(pgstattuple(to_regclass(Expr::val(relation))), Alias::new("physical"))
        .to_owned()
}

/// Every invalid index in the application schema and the published Snapshot
/// branch schemas.
fn invalid_indexes_statement() -> SelectStatement {
    let schema = || Expr::col((Alias::new("n"), Alias::new("nspname")));
    Query::select()
        .column((Alias::new("n"), Alias::new("nspname")))
        .column((Alias::new("c"), Alias::new("relname")))
        .from_as(Alias::new("pg_index"), Alias::new("i"))
        .join_as(
            JoinType::InnerJoin,
            Alias::new("pg_class"),
            Alias::new("c"),
            Expr::col((Alias::new("c"), Alias::new("oid"))).equals((Alias::new("i"), Alias::new("indexrelid"))),
        )
        .join_as(
            JoinType::InnerJoin,
            Alias::new("pg_namespace"),
            Alias::new("n"),
            Expr::col((Alias::new("n"), Alias::new("oid"))).equals((Alias::new("c"), Alias::new("relnamespace"))),
        )
        .and_where(schema().eq("public").or(published_branch_schema(schema())))
        .and_where(Expr::col((Alias::new("i"), Alias::new("indisvalid"))).eq(false))
        .order_by((Alias::new("n"), Alias::new("nspname")), sea_query::Order::Asc)
        .order_by((Alias::new("c"), Alias::new("relname")), sea_query::Order::Asc)
        .to_owned()
}

/// Every published Snapshot branch schema.
fn published_branches_statement() -> SelectStatement {
    Query::select()
        .column(Alias::new(CatalogColumn::Nspname.as_str()))
        .from(Alias::new("pg_namespace"))
        .and_where(published_branch_schema(Expr::col(Alias::new(CatalogColumn::Nspname.as_str()))))
        .to_owned()
}

/// Whether a schema name is a published Snapshot branch's, never a staging
/// one's.
fn published_branch_schema(schema: Expr) -> SimpleExpr {
    schema.binary(PgBinOper::Regex, Expr::val(PUBLISHED_SNAPSHOT_SCHEMA_PATTERN))
}

/// Stable projected field names decoded from catalog rows.
#[derive(Clone, Copy)]
enum CatalogColumn {
    /// `pg_stat_user_indexes.indexrelname` projection.
    Indexrelname,
    /// Heap or index relation-name projection.
    Relname,
    /// Schema-name projection.
    Nspname,
    /// Physical relation byte projection.
    Bytes,
    /// Detailed index free-byte projection.
    FreeSpace,
    /// Detailed index physical-length projection.
    TableLength,
}

impl CatalogColumn {
    /// Render the exact projected field name.
    const fn as_str(self) -> &'static str {
        match self {
            CatalogColumn::Indexrelname => "indexrelname",
            CatalogColumn::Relname => "relname",
            CatalogColumn::Nspname => "nspname",
            CatalogColumn::Bytes => "bytes",
            CatalogColumn::FreeSpace => "free_space",
            CatalogColumn::TableLength => "table_len",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        branch::Branch,
        candidate::index_target::IndexTarget,
        inspection::{index_sizes_statement, invalid_indexes_statement, published_branches_statement},
    };
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn catalog_queries_are_seaquery_statements_with_closed_filters() {
        let index_sql = index_sizes_statement(Branch::Live).to_string(PostgresQueryBuilder);
        assert!(index_sql.contains("pg_stat_user_indexes"), "{index_sql}");
        assert!(index_sql.contains("idx_ca_value_string_trgm"), "{index_sql}");
        let invalid_sql = invalid_indexes_statement().to_string(PostgresQueryBuilder);
        assert!(invalid_sql.contains("pg_index"), "{invalid_sql}");
        assert!(invalid_sql.contains("indisvalid"), "{invalid_sql}");
    }

    /// The size sample reads the GIN indexes too: a density is computed from
    /// it, and a queued GIN reports its bytes before and after its rebuild.
    #[test]
    fn index_size_sample_filters_by_heap_and_index_pairs() {
        let sql = index_sizes_statement(Branch::Live).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"("relname", "indexrelname") IN ("#), "{sql}");
        for target in IndexTarget::ALL {
            let pair = format!("('{}', '{}')", target.heap().relation_name(), target.relation_name());
            assert!(sql.contains(&pair), "{target}: {sql}");
        }
        assert!(!sql.contains(r#""relname" = 'current_attributes'"#), "{sql}");
    }

    #[test]
    fn a_snapshot_branch_index_size_sample_reads_its_schema() {
        let branch = Branch::Snapshot(SnapshotKey::try_from(7).expect("positive snapshot key"));
        let indexes = index_sizes_statement(branch).to_string(PostgresQueryBuilder);
        assert!(indexes.contains(r#""schemaname" = 'snap_7'"#), "{indexes}");
    }

    #[test]
    fn invalid_index_statement_covers_public_and_published_branch_schemas() {
        let sql = invalid_indexes_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"SELECT "n"."nspname", "c"."relname""#), "{sql}");
        assert!(sql.contains(r#"("n"."nspname" = 'public' OR ("n"."nspname" ~ '^snap_[1-9][0-9]*$'))"#), "{sql}");
        assert!(!sql.contains("_stg"), "{sql}");
    }

    #[test]
    fn the_published_branch_read_matches_only_published_snapshot_schemas() {
        let sql = published_branches_statement().to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT "nspname" FROM "pg_namespace" WHERE "nspname" ~ '^snap_[1-9][0-9]*$'"#);
    }
}

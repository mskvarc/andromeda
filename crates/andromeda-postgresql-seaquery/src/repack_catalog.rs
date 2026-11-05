//! The catalog reads around a repack: what an interrupted run left in a
//! tenant database, and the free space a repack would give back.
//!
//! A run's log and copy tables live in the `repack` schema as `log_<oid>` and
//! `table_<oid>`, and its trigger on the table it repacks; the reads here
//! find all three from the catalog alone. The free space of a heap or of its
//! TOAST table is estimated by `pgstattuple_approx`, from the `pgstattuple`
//! extension, which reads only the pages the visibility map does not mark
//! all-visible.

use crate::{
    cast::cast,
    maintenance::to_regclass,
    pg_type::PostgresType,
    repack::{REPACK_TRIGGER, oid},
};
use sea_query::{Alias, Expr, ExprTrait, Func, Iden, Order, Query, SelectStatement};

/// The catalog relations and columns the reads name.
#[derive(Clone, Copy, Iden)]
enum CatalogRelation {
    /// `pg_class`.
    #[iden = "pg_class"]
    PgClass,
    /// `pg_class.oid`.
    Oid,
    /// `pg_class.relname`.
    Relname,
    /// `pg_class.relkind`.
    Relkind,
    /// `pg_class.relnamespace`.
    Relnamespace,
    /// `pg_class.reltoastrelid`.
    Reltoastrelid,
    /// `pg_namespace`.
    #[iden = "pg_namespace"]
    PgNamespace,
    /// `pg_namespace.nspname`.
    Nspname,
    /// `pg_trigger`.
    #[iden = "pg_trigger"]
    PgTrigger,
    /// `pg_trigger.tgrelid`.
    Tgrelid,
    /// `pg_trigger.tgname`.
    Tgname,
}

/// `pgstattuple_approx(relation)`, from the `pgstattuple` extension.
#[derive(Clone, Copy, Iden)]
#[iden = "pgstattuple_approx"]
struct PgstattupleApprox;

/// The columns [`approximate_free_space`] reads, in its column order.
#[derive(Clone, Copy, Debug, Iden)]
pub enum ApproximateFreeSpace {
    /// The relation's physical length in bytes.
    TableLen,
    /// Bytes the free space map records as free, estimated.
    ApproxFreeSpace,
    /// Dead tuples' share of the relation, as a percentage.
    DeadTuplePercent,
}

/// The name of every ordinary table in the `repack` schema; a run's log and
/// copy tables are named `log_<oid>` and `table_<oid>`.
#[must_use]
pub fn repack_schema_tables() -> SelectStatement {
    Query::select()
        .column((CatalogRelation::PgClass, CatalogRelation::Relname))
        .from(CatalogRelation::PgClass)
        .inner_join(
            CatalogRelation::PgNamespace,
            Expr::col((CatalogRelation::PgNamespace, CatalogRelation::Oid)).equals((CatalogRelation::PgClass, CatalogRelation::Relnamespace)),
        )
        .and_where(Expr::col((CatalogRelation::PgNamespace, CatalogRelation::Nspname)).eq("repack"))
        .and_where(Expr::col((CatalogRelation::PgClass, CatalogRelation::Relkind)).eq("r"))
        .order_by((CatalogRelation::PgClass, CatalogRelation::Relname), Order::Asc)
        .to_owned()
}

/// The OID of every table carrying a run's trigger, as a `bigint`.
#[must_use]
pub fn triggered_tables() -> SelectStatement {
    Query::select()
        .expr(cast(Expr::col(CatalogRelation::Tgrelid), PostgresType::Bigint))
        .from(CatalogRelation::PgTrigger)
        .and_where(Expr::col(CatalogRelation::Tgname).eq(REPACK_TRIGGER))
        .order_by(CatalogRelation::Tgrelid, Order::Asc)
        .to_owned()
}

/// The schema and name of the table `relid`, no row when none has the OID.
#[must_use]
pub fn relation_identity(relid: u32) -> SelectStatement {
    Query::select()
        .column((CatalogRelation::PgNamespace, CatalogRelation::Nspname))
        .column((CatalogRelation::PgClass, CatalogRelation::Relname))
        .from(CatalogRelation::PgClass)
        .inner_join(
            CatalogRelation::PgNamespace,
            Expr::col((CatalogRelation::PgNamespace, CatalogRelation::Oid)).equals((CatalogRelation::PgClass, CatalogRelation::Relnamespace)),
        )
        .and_where(Expr::col((CatalogRelation::PgClass, CatalogRelation::Oid)).eq(oid(relid)))
        .to_owned()
}

/// The OID of `relation` and of its TOAST table, both as `bigint`, the TOAST
/// OID 0 for a table without one; no row when `relation` does not resolve.
#[must_use]
pub fn relation_and_toast(relation: &str) -> SelectStatement {
    Query::select()
        .expr(cast(Expr::col(CatalogRelation::Oid), PostgresType::Bigint))
        .expr(cast(Expr::col(CatalogRelation::Reltoastrelid), PostgresType::Bigint))
        .from(CatalogRelation::PgClass)
        .and_where(Expr::col(CatalogRelation::Oid).eq(to_regclass(Expr::val(relation))))
        .to_owned()
}

/// `SELECT table_len, approx_free_space, dead_tuple_percent::integer FROM
/// pgstattuple_approx(relid::regclass)`: the free space of a heap or a TOAST
/// table, estimated from its visibility and free space maps, reading only the
/// pages not marked all-visible; the dead share rounded to a whole percent.
#[must_use]
pub fn approximate_free_space(relid: u32) -> SelectStatement {
    Query::select()
        .columns([ApproximateFreeSpace::TableLen, ApproximateFreeSpace::ApproxFreeSpace])
        .expr(cast(Expr::col(ApproximateFreeSpace::DeadTuplePercent), PostgresType::Integer))
        .from_function(
            Func::cust(PgstattupleApprox).arg(cast(oid(relid), PostgresType::Regclass)),
            Alias::new("free_space"),
        )
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::repack_catalog::{approximate_free_space, relation_and_toast, relation_identity, repack_schema_tables, triggered_tables};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn leftovers_are_read_from_the_repack_schema_and_the_trigger_catalog() {
        assert_eq!(
            repack_schema_tables().to_string(PostgresQueryBuilder),
            r#"SELECT "pg_class"."relname" FROM "pg_class" INNER JOIN "pg_namespace" ON "pg_namespace"."oid" = "pg_class"."relnamespace" WHERE "pg_namespace"."nspname" = 'repack' AND "pg_class"."relkind" = 'r' ORDER BY "pg_class"."relname" ASC"#
        );
        assert_eq!(
            triggered_tables().to_string(PostgresQueryBuilder),
            r#"SELECT "tgrelid"::bigint FROM "pg_trigger" WHERE "tgname" = 'repack_trigger' ORDER BY "tgrelid" ASC"#
        );
        assert_eq!(
            relation_identity(16_384).to_string(PostgresQueryBuilder),
            r#"SELECT "pg_namespace"."nspname", "pg_class"."relname" FROM "pg_class" INNER JOIN "pg_namespace" ON "pg_namespace"."oid" = "pg_class"."relnamespace" WHERE "pg_class"."oid" = (16384::oid)"#
        );
    }

    #[test]
    fn the_free_space_probe_reads_a_heap_and_its_toast_table_by_oid() {
        assert_eq!(
            relation_and_toast("snap_7.notification_deliveries").to_string(PostgresQueryBuilder),
            r#"SELECT "oid"::bigint, "reltoastrelid"::bigint FROM "pg_class" WHERE "oid" = to_regclass('snap_7.notification_deliveries')"#
        );
        assert_eq!(
            approximate_free_space(16_384).to_string(PostgresQueryBuilder),
            r#"SELECT "table_len", "approx_free_space", "dead_tuple_percent"::integer FROM pgstattuple_approx(16384::oid::regclass) AS "free_space""#
        );
    }
}

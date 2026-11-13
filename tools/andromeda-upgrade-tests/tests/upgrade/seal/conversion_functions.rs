//! The upgrade leaves in `public` only the `seal_conversion_` functions a
//! Snapshot branch created later calls again. The two that convert the queues
//! are temporary, so they never reach the schema.

use crate::fixtures::{
    rehearsal::{at_v1_0_0, intern_seal_attribute},
    statements::name,
    test_error::UpgradeTestError,
};
use andromeda_postgresql_seaquery::statement::Statement;
use pretty_assertions::assert_eq;
use sea_query::{Expr, ExprTrait, JoinType, Order, Query};
use sqlx::PgPool;

/// The functions of `public` whose names start with `seal_conversion_`, by
/// name. A backslash escapes the underscores, which LIKE would otherwise take
/// for any one character.
async fn conversion_functions(pool: &PgPool) -> Result<Vec<String>, UpgradeTestError> {
    let function = name("function");
    let namespace = name("namespace");
    let select = Query::select()
        .column((function.clone(), name("proname")))
        .from_as((name("pg_catalog"), name("pg_proc")), function.clone())
        .join_as(
            JoinType::InnerJoin,
            (name("pg_catalog"), name("pg_namespace")),
            namespace.clone(),
            Expr::col((namespace.clone(), name("oid"))).equals((function.clone(), name("pronamespace"))),
        )
        .and_where(Expr::col((namespace, name("nspname"))).eq("public"))
        .and_where(Expr::col((function.clone(), name("proname"))).like(r"seal\_conversion\_%"))
        .order_by((function, name("proname")), Order::Asc)
        .to_owned();
    Ok(select.to_query_scalar::<String>().fetch_all(pool).await?)
}

#[tokio::test]
async fn the_upgrade_leaves_in_public_only_the_conversion_functions_a_later_snapshot_branch_calls() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    intern_seal_attribute(database.pool()).await?;

    database.migrate_to_current().await?;

    assert_eq!(
        conversion_functions(database.pool()).await?,
        [
            "seal_conversion_admits_uri",
            "seal_conversion_attribute_key",
            "seal_conversion_entity_id",
            "seal_conversion_entity_type_names",
            "seal_conversion_intern_entity_types",
            "seal_conversion_kept_changes",
            "seal_conversion_plain_string",
            "seal_conversion_proof",
            "seal_conversion_reference",
            "seal_conversion_state_holds_seal",
            "seal_conversion_typed_state",
            "seal_conversion_typed_string",
            "seal_conversion_typed_tree",
        ]
    );
    database.drop_database().await?;
    Ok(())
}

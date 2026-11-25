//! The rewrite of migration 0044 on its own, in a tenant stopped after it:
//! each path text v1.0.0 wrote becomes the text with each unquoted name
//! holding a colon in double quotes, and the rewrite applied twice gives what
//! it gives once.

use crate::{
    fixtures::{rehearsal::at_v1_0_0, test_error::UpgradeTestError},
    query_name::rewrite_cases::REWRITES,
};
use andromeda_postgresql_context_store::migration_set::migration_version::MigrationVersion;
use andromeda_postgresql_seaquery::{query_name_rewrite::path_text_with_colon_names_quoted, statement::Statement};
use pretty_assertions::assert_eq;
use sea_query::{Expr, Query};
use sqlx::PgPool;

/// The tenant migration that creates the `query_name_rewrite` schema. The
/// upgrade drops it again, so the rewrite is judged with the tenant stopped
/// here.
const REWRITE: MigrationVersion = MigrationVersion::new(44);

/// The text the rewrite makes of `path`.
async fn rewritten(pool: &PgPool, path: &str) -> Result<String, UpgradeTestError> {
    let select = Query::select().expr(path_text_with_colon_names_quoted(Expr::val(path))).to_owned();
    Ok(select.to_query_scalar::<String>().fetch_one(pool).await?)
}

#[tokio::test]
async fn the_rewrite_quotes_each_unquoted_name_holding_a_colon_and_nothing_else() -> Result<(), UpgradeTestError> {
    let (database, _) = at_v1_0_0().await?;
    database.migrate_public_through(REWRITE).await?;

    for (written, expected) in REWRITES {
        let once = rewritten(database.pool(), written).await?;
        assert_eq!(once, expected, "{written}");
        assert_eq!(rewritten(database.pool(), &once).await?, once, "{written} rewritten twice");
    }
    database.drop_database().await?;
    Ok(())
}

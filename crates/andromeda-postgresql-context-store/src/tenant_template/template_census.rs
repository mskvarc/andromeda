//! The templates of one prefix the server holds, and whether each can be
//! copied.

use crate::{error::ContextStoreError, tenant_template::template_name::family_start};
use andromeda_configuration::schema::db_name_prefix::DbNamePrefix;
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use andromeda_postgresql_pool::database_url::DatabaseUrl;
use andromeda_postgresql_seaquery::{statement::Statement, system_catalog::PgDatabase};
use sea_query::{Expr, ExprTrait, LikeExpr, Query, SelectStatement};
use sqlx::{Connection, PgConnection};

/// The character the census pattern escapes `LIKE` wildcards with.
const LIKE_ESCAPE: char = '\\';

/// Whether a template database can be copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TemplateReadiness {
    /// Marked a template and refusing connections, as a finished build leaves
    /// it.
    Ready,
    /// Anything else: a build cut short, or a template an operator changed.
    Unfinished,
}

/// One template database of the prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TemplateDatabase {
    /// The database's name.
    pub(crate) name: TenantDatabaseName,
    /// Whether it can be copied.
    pub(crate) readiness: TemplateReadiness,
}

/// Every database the server holds whose name starts as a template of
/// `prefix` does, over a connection to the database `direct_url` names.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the connection cannot be
/// opened or closed, or the catalogue read fails.
pub(crate) async fn template_census(direct_url: &DatabaseUrl, prefix: &DbNamePrefix) -> Result<Vec<TemplateDatabase>, ContextStoreError> {
    let mut connection = PgConnection::connect(direct_url.as_ref().as_str()).await?;
    let rows = census_statement(prefix).to_query_as::<(String, bool, bool)>().fetch_all(&mut connection).await;
    connection.close().await?;
    Ok(rows?
        .into_iter()
        .map(|(name, is_template, allows_connections)| TemplateDatabase {
            name: TenantDatabaseName::from(name),
            readiness: readiness(is_template, allows_connections),
        })
        .collect())
}

/// Whether a database marked a template or not, and admitting connections or
/// not, can be copied.
const fn readiness(is_template: bool, allows_connections: bool) -> TemplateReadiness {
    match (is_template, allows_connections) {
        (true, false) => TemplateReadiness::Ready,
        (true, true) | (false, true | false) => TemplateReadiness::Unfinished,
    }
}

/// `SELECT datname, datistemplate, datallowconn FROM pg_database WHERE datname
/// LIKE '<family start>%'`, the family start escaped so the `_` a prefix holds
/// matches only itself.
fn census_statement(prefix: &DbNamePrefix) -> SelectStatement {
    let pattern = format!("{}%", like_literal(&family_start(prefix)));
    Query::select()
        .columns([PgDatabase::Datname, PgDatabase::Datistemplate, PgDatabase::Datallowconn])
        .from(PgDatabase::Table)
        .and_where(Expr::col(PgDatabase::Datname).like(LikeExpr::new(pattern).escape(LIKE_ESCAPE)))
        .to_owned()
}

/// `text` as a `LIKE` pattern that matches only `text`.
fn like_literal(text: &str) -> String {
    text.chars().fold(String::with_capacity(text.len()), |mut pattern, character| {
        if matches!(character, '%' | '_') || character == LIKE_ESCAPE {
            pattern.push(LIKE_ESCAPE);
        }
        pattern.push(character);
        pattern
    })
}

#[cfg(test)]
mod tests {
    use crate::tenant_template::template_census::{TemplateReadiness, census_statement, like_literal, readiness};
    use andromeda_configuration::schema::db_name_prefix::DbNamePrefix;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn only_a_marked_template_that_refuses_connections_is_ready() {
        assert_eq!(readiness(true, false), TemplateReadiness::Ready);
        assert_eq!(readiness(true, true), TemplateReadiness::Unfinished);
        assert_eq!(readiness(false, true), TemplateReadiness::Unfinished);
        assert_eq!(readiness(false, false), TemplateReadiness::Unfinished);
    }

    #[test]
    fn a_wildcard_in_the_family_start_matches_only_itself() {
        assert_eq!(like_literal("andromeda_tenant-template-"), r"andromeda\_tenant-template-");
        assert_eq!(like_literal(r"a%b\c"), r"a\%b\\c");
    }

    #[test]
    fn the_census_reads_the_flags_of_the_databases_named_like_the_prefixs_templates() {
        let (sql, values) = census_statement(&DbNamePrefix::default()).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT "datname", "datistemplate", "datallowconn" FROM "pg_database" WHERE "datname" LIKE $1 ESCAPE E'\\'"#
        );
        assert_eq!(values.0.len(), 1, "the pattern is bound: {sql}");
        assert!(format!("{values:?}").contains(r"andromeda\\_tenant-template-%"), "{values:?}");
    }
}

//! Active PostgreSQL-family component discovery.

use andromeda_component_report::{ComponentReport, ComponentVersion, TimescaleLicense};
use andromeda_postgresql_seaquery::introspection::{current_setting, current_setting_missing_ok, extension_version};
use sea_query::{Alias, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Row};

/// Discover the PostgreSQL server and installed storage extensions active on one pool.
pub async fn discover(pool: &PgPool) -> Result<Vec<ComponentReport>, sqlx::Error> {
    let (sql, values) = discover_statement().build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(pool).await?;

    let pg_version: String = row.get("pg_version");
    let postgis_version: Option<String> = row.get("postgis_version");
    let timescaledb_version: Option<String> = row.get("timescaledb_version");
    let timescaledb_license: Option<String> = row.get("timescaledb_license");
    let mut reports = vec![ComponentReport::PostgreSql {
        version: ComponentVersion::from(format!("v{pg_version}")),
    }];
    if let Some(version) = postgis_version {
        reports.push(ComponentReport::PostGis {
            version: ComponentVersion::from(format!("v{version}")),
        });
    }
    if let Some(version) = timescaledb_version {
        reports.push(ComponentReport::TimescaleDb {
            version: ComponentVersion::from(format!("v{version}")),
            license: TimescaleLicense::from(normalize_timescale_license(timescaledb_license.as_deref())),
        });
    }
    Ok(reports)
}

/// The introspection query reading the server version, the PostGIS and
/// TimescaleDB extension versions, and the TimescaleDB license marker.
///
/// Each column is a system-function call or a `pg_extension` lookup, so the
/// statement names no application relation; the extension and setting names it
/// reads are bound values, not interpolated text.
fn discover_statement() -> sea_query::SelectStatement {
    Query::select()
        .expr_as(current_setting("server_version"), Alias::new("pg_version"))
        .expr_as(extension_version("postgis"), Alias::new("postgis_version"))
        .expr_as(extension_version("timescaledb"), Alias::new("timescaledb_version"))
        .expr_as(current_setting_missing_ok("timescaledb.license"), Alias::new("timescaledb_license"))
        .to_owned()
}

/// Normalize TimescaleDB's setting values into stable report values.
fn normalize_timescale_license(license: Option<&str>) -> String {
    match license {
        Some("timescale") => "TSL".to_owned(),
        Some("apache") => "Apache-2.0".to_owned(),
        Some(other) => other.to_owned(),
        None => "unknown".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use crate::component_report::{discover_statement, normalize_timescale_license};
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_discovery_reads_the_server_and_both_extensions() {
        let sql = discover_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"current_setting('server_version') AS "pg_version""#), "{sql}");
        assert!(sql.contains(r#""pg_extension""#), "{sql}");
        assert!(sql.contains(r#"AS "postgis_version""#), "{sql}");
        assert!(sql.contains(r#"AS "timescaledb_version""#), "{sql}");
        assert!(
            sql.contains(r#"current_setting('timescaledb.license', TRUE) AS "timescaledb_license""#),
            "{sql}"
        );
    }

    #[test]
    fn known_timescale_license_settings_use_stable_names() {
        pretty_assertions::assert_eq!(normalize_timescale_license(Some("timescale")), "TSL");
        pretty_assertions::assert_eq!(normalize_timescale_license(Some("apache")), "Apache-2.0");
    }

    #[test]
    fn absent_timescale_license_is_reported_without_inventing_a_license() {
        pretty_assertions::assert_eq!(normalize_timescale_license(None), "unknown");
    }
}

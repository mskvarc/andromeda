//! Active PostgreSQL-family component discovery.

use andromeda_component_report::{ComponentReport, ComponentVersion, TimescaleLicense};
use sqlx::{PgPool, Row};

/// Discover the PostgreSQL server and installed storage extensions active on one pool.
pub async fn discover(pool: &PgPool) -> Result<Vec<ComponentReport>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT \
            current_setting('server_version') AS pg_version, \
            (SELECT extversion FROM pg_extension WHERE extname = 'postgis') AS postgis_version, \
            (SELECT extversion FROM pg_extension WHERE extname = 'timescaledb') AS timescaledb_version, \
            current_setting('timescaledb.license', true) AS timescaledb_license",
    )
    .fetch_one(pool)
    .await?;

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
    use crate::component_report::normalize_timescale_license;

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

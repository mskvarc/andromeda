//! Active PostgreSQL-family component discovery.

use andromeda_component_report::{ComponentReport, ComponentVersion, ConnectionCapacity, TimescaleLicense};
use andromeda_postgresql_seaquery::{
    cast::cast,
    introspection::{current_role_is_member_of, current_role_is_superuser, current_setting, current_setting_missing_ok, extension_version},
    pg_type::PostgresType,
};
use sea_query::{Alias, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Row};

/// The predefined role whose members may take the `reserved_connections`
/// slots (PostgreSQL 16 and later).
const RESERVED_CONNECTIONS_ROLE: &str = "pg_use_reserved_connections";

/// The connection slots the session's role may not take.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReservedSlots {
    /// `superuser_reserved_connections`.
    superuser: u32,
    /// `reserved_connections`; zero on a server that has no such setting.
    reserved: u32,
    /// Whether the role is a superuser, which may take every slot.
    is_superuser: bool,
    /// Whether the role is a member of [`RESERVED_CONNECTIONS_ROLE`].
    uses_reserved: bool,
}

impl ReservedSlots {
    /// The slots kept from the role.
    const fn kept_from_role(self) -> u32 {
        if self.is_superuser {
            0
        } else if self.uses_reserved {
            self.superuser
        } else {
            self.superuser.saturating_add(self.reserved)
        }
    }
}

/// Discover the PostgreSQL server and installed storage extensions active on
/// one pool, with the server's connection limit set against
/// `broker_ceiling`, the connections every pool of the broker holds when all
/// are full at once.
///
/// # Errors
///
/// Returns [`sqlx::Error`] when the introspection statement cannot be executed
/// against the pool, or when the returned row does not carry the expected
/// version, license and connection columns.
pub async fn discover(pool: &PgPool, broker_ceiling: u32) -> Result<Vec<ComponentReport>, sqlx::Error> {
    let (sql, values) = discover_statement().build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(pool).await?;

    let pg_version: String = row.try_get("pg_version")?;
    let postgis_version: Option<String> = row.try_get("postgis_version")?;
    let timescaledb_version: Option<String> = row.try_get("timescaledb_version")?;
    let timescaledb_license: Option<String> = row.try_get("timescaledb_license")?;
    let max_connections: i32 = row.try_get("max_connections")?;
    let slots = ReservedSlots {
        superuser: setting_count(row.try_get("superuser_reserved_connections")?),
        reserved: setting_count(row.try_get::<Option<i32>, _>("reserved_connections")?.unwrap_or_default()),
        is_superuser: row.try_get::<Option<bool>, _>("is_superuser")?.unwrap_or_default(),
        uses_reserved: row.try_get::<Option<bool>, _>("uses_reserved_connections")?.unwrap_or_default(),
    };
    let mut reports = vec![ComponentReport::PostgreSql {
        version: ComponentVersion::from(format!("v{pg_version}")),
        connections: ConnectionCapacity::new(setting_count(max_connections), slots.kept_from_role(), broker_ceiling),
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

/// A connection-count setting as a count; PostgreSQL bounds each at zero or
/// above.
fn setting_count(value: i32) -> u32 {
    u32::try_from(value).unwrap_or_default()
}

/// The introspection query reading the server version, the `PostGIS` and
/// `TimescaleDB` extension versions, the `TimescaleDB` license marker, the
/// connection limit with its reserved slots, and whether the session's role
/// may take them.
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
        .expr_as(cast(current_setting("max_connections"), PostgresType::Integer), Alias::new("max_connections"))
        .expr_as(
            cast(current_setting("superuser_reserved_connections"), PostgresType::Integer),
            Alias::new("superuser_reserved_connections"),
        )
        .expr_as(
            cast(current_setting_missing_ok("reserved_connections"), PostgresType::Integer),
            Alias::new("reserved_connections"),
        )
        .expr_as(current_role_is_superuser(), Alias::new("is_superuser"))
        .expr_as(current_role_is_member_of(RESERVED_CONNECTIONS_ROLE), Alias::new("uses_reserved_connections"))
        .to_owned()
}

/// Normalize `TimescaleDB`'s setting values into stable report values.
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
    use crate::component_report::{ReservedSlots, discover_statement, normalize_timescale_license};
    use pretty_assertions::assert_eq;
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
    fn the_discovery_reads_the_connection_limit_and_the_slots_the_role_may_take() {
        let sql = discover_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"current_setting('max_connections')::integer AS "max_connections""#), "{sql}");
        assert!(
            sql.contains(r#"current_setting('reserved_connections', TRUE)::integer AS "reserved_connections""#),
            "{sql}"
        );
        assert!(sql.contains(r#"AS "is_superuser""#), "{sql}");
        assert!(sql.contains("'pg_use_reserved_connections'"), "{sql}");
    }

    /// A superuser may take every slot; a member of the reserved role every
    /// slot but the superuser ones; any other role neither.
    #[test]
    fn the_slots_kept_from_the_role_follow_its_privileges() {
        let slots = |is_superuser, uses_reserved| ReservedSlots {
            superuser: 3,
            reserved: 5,
            is_superuser,
            uses_reserved,
        };
        assert_eq!(slots(false, false).kept_from_role(), 8);
        assert_eq!(slots(false, true).kept_from_role(), 3);
        assert_eq!(slots(true, false).kept_from_role(), 0);
        assert_eq!(slots(true, true).kept_from_role(), 0);
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

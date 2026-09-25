//! The server and extension versions the tenant schema depends on.

use crate::schema_contract::error::SchemaContractError;
use andromeda_postgresql_seaquery::{
    cast::cast,
    introspection::{current_setting, extension_version},
    pg_type::PostgresType,
};
use sea_query::{Alias, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Row};

/// The PostgreSQL version the tenant schema requires.
///
/// `temporal_attribute_streams` identifies a multi-attribute instance with
/// `UNIQUE NULLS NOT DISTINCT (temporal_entity_key, attribute_name_id, dataset_id)`,
/// which is how ETSI GS CIM 009 v1.9.1 clause 4.5.5 is modelled — an absent
/// `datasetId` is the one default instance — and `NULLS NOT DISTINCT` is a
/// PostgreSQL 15 feature. The floor rises to 18 when the schema adopts
/// database-side `uuidv7()` defaults; until it does, stating 18 here would
/// refuse servers the schema runs on correctly.
pub const REQUIRED_POSTGRES_VERSION: PostgresVersion = PostgresVersion::new(150_000);

/// The `PostGIS` version the tenant schema requires.
///
/// The schema stores `geometry(PointZ, 4326)` and casts to `geography` for
/// distance predicates, and the compiled `georel` predicates call
/// `ST_MakeValid`; all of that is `PostGIS` 3.0 surface.
pub const REQUIRED_POSTGIS_VERSION: ExtensionVersion = ExtensionVersion::new(3, 0);

/// The `TimescaleDB` version the tenant schema requires.
///
/// `attribute_history` and `scope_history` are hypertables created with
/// `create_hypertable(..., by_range(...))`, chunk-managed with
/// `set_chunk_time_interval` and aged out with `drop_chunks` — all Apache-2.0
/// core surface. The floor is 2.18 because the TSL columnstore path the TSL
/// edition additionally takes — `enable_columnstore`, `add_columnstore_policy` —
/// is 2.18 surface, and both editions ship the same package version, so one
/// floor covers both.
///
/// The floor is a version floor only, never a license floor. The Apache-2.0
/// core is fully supported: its migrations skip the columnstore DDL behind a
/// `pg_proc` capability probe and its hypertables run uncompressed, so no
/// edition is refused here. The columnstore is a TSL benefit that rides along
/// where present, not a requirement.
pub const REQUIRED_TIMESCALEDB_VERSION: ExtensionVersion = ExtensionVersion::new(2, 18);

/// PostgreSQL's `server_version_num`: `major * 10000 + minor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display)]
#[display("{}.{}", _0 / 10_000, _0 % 10_000)]
pub struct PostgresVersion(i32);

impl PostgresVersion {
    /// Wrap what `server_version_num` reports.
    #[must_use]
    pub const fn new(server_version_num: i32) -> PostgresVersion {
        PostgresVersion(server_version_num)
    }
}

/// The `major.minor` prefix of an installed extension's version.
///
/// Field order is the comparison order: `Ord` is derived, so major dominates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display)]
#[display("{major}.{minor}")]
pub struct ExtensionVersion {
    /// The major component, compared first.
    pub major: u32,
    /// The minor component, compared second.
    pub minor: u32,
}

impl ExtensionVersion {
    /// Name a `major.minor` version.
    #[must_use]
    pub const fn new(major: u32, minor: u32) -> ExtensionVersion {
        ExtensionVersion { major, minor }
    }
}

impl TryFrom<&str> for ExtensionVersion {
    type Error = ExtensionVersionError;

    /// Read what `pg_extension.extversion` holds.
    ///
    /// Only the leading digit run of each component is read, because a
    /// development build reports a component such as `18-dev`; a component with
    /// no leading digit is refused rather than assumed to be zero, since a
    /// version that cannot be read cannot be shown to meet a floor.
    fn try_from(value: &str) -> Result<ExtensionVersion, ExtensionVersionError> {
        let unreadable = || ExtensionVersionError::Unreadable { found: value.to_owned() };
        let mut components = value.split('.');
        let (Some(major), Some(minor)) = (components.next(), components.next()) else {
            return Err(unreadable());
        };
        Ok(ExtensionVersion {
            major: leading_number(major).ok_or_else(unreadable)?,
            minor: leading_number(minor).ok_or_else(unreadable)?,
        })
    }
}

/// The number the leading ASCII-digit run of a version component spells.
fn leading_number(component: &str) -> Option<u32> {
    let digits: &str = component.split(|character: char| !character.is_ascii_digit()).next()?;
    digits.parse().ok()
}

/// Whether a required extension is installed, and at what version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display)]
pub enum ExtensionPresence {
    /// The extension is not installed in this database.
    #[display("absent")]
    Absent,
    /// The extension is installed at this version.
    #[display("{_0}")]
    Installed(ExtensionVersion),
}

impl TryFrom<Option<String>> for ExtensionPresence {
    type Error = ExtensionVersionError;

    /// Read one `pg_extension.extversion` lookup, which is `NULL` exactly when
    /// the extension is not installed.
    fn try_from(extversion: Option<String>) -> Result<ExtensionPresence, ExtensionVersionError> {
        match extversion {
            None => Ok(ExtensionPresence::Absent),
            Some(raw) => ExtensionVersion::try_from(raw.as_str()).map(ExtensionPresence::Installed),
        }
    }
}

/// What refuses a value the extension-version domain cannot hold.
#[derive(Debug, thiserror::Error)]
pub enum ExtensionVersionError {
    /// The reported version has no readable `major.minor` prefix.
    #[error("extension version '{found}' has no readable major.minor prefix")]
    Unreadable {
        /// The version the catalogue reports.
        found: String,
    },
}

/// Refuse a tenant whose server or extensions are below what the schema needs.
///
/// Only the two versioned extensions are checked here. The contrib extensions
/// the schema also creates — `pg_trgm`, `ltree`, `btree_gist`, `btree_gin`
/// (the `int4_ops` half of the attribute-aware trigram index on
/// `current_attribute_values`) — ship with every supported server, so their
/// `CREATE EXTENSION IF NOT EXISTS` in migration 0006 is the check.
///
/// # Errors
///
/// Returns [`SchemaContractError::Database`] when the introspection query
/// fails, [`SchemaContractError::ExtensionVersionValue`] when a reported
/// extension version cannot be read, and
/// [`SchemaContractError::PostgresBelowMinimum`],
/// [`SchemaContractError::PostGisBelowMinimum`] or
/// [`SchemaContractError::TimescaleDbBelowMinimum`] when the server or one of
/// the two versioned extensions is below the floor the schema depends on.
pub async fn verify_server_requirements(pool: &PgPool) -> Result<(), SchemaContractError> {
    let (sql, values) = server_requirements_statement().build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(pool).await?;

    check_postgres(PostgresVersion::new(row.get("server_version_num")))?;
    check_extension(
        REQUIRED_POSTGIS_VERSION,
        ExtensionPresence::try_from(row.get::<Option<String>, _>("postgis_version"))?,
        |required, found| SchemaContractError::PostGisBelowMinimum { required, found },
    )?;
    check_extension(
        REQUIRED_TIMESCALEDB_VERSION,
        ExtensionPresence::try_from(row.get::<Option<String>, _>("timescaledb_version"))?,
        |required, found| SchemaContractError::TimescaleDbBelowMinimum { required, found },
    )
}

/// Read the server's `server_version_num` and the `PostGIS` and `TimescaleDB`
/// extension versions.
///
/// Every column is a system-function call or a `pg_extension` lookup, so the
/// statement names no application relation and binds the setting and extension
/// names it reads.
fn server_requirements_statement() -> SelectStatement {
    Query::select()
        .expr_as(
            cast(current_setting("server_version_num"), PostgresType::Integer),
            Alias::new("server_version_num"),
        )
        .expr_as(extension_version("postgis"), Alias::new("postgis_version"))
        .expr_as(extension_version("timescaledb"), Alias::new("timescaledb_version"))
        .to_owned()
}

/// Compare the server against its floor.
fn check_postgres(found: PostgresVersion) -> Result<(), SchemaContractError> {
    if found < REQUIRED_POSTGRES_VERSION {
        return Err(SchemaContractError::PostgresBelowMinimum {
            required: REQUIRED_POSTGRES_VERSION,
            found,
        });
    }
    Ok(())
}

/// Compare one extension against its floor, refusing an absent one the same way
/// as one that is too old: neither can run the schema.
fn check_extension<Shortfall>(required: ExtensionVersion, found: ExtensionPresence, shortfall: Shortfall) -> Result<(), SchemaContractError>
where
    Shortfall: FnOnce(ExtensionVersion, ExtensionPresence) -> SchemaContractError,
{
    match found {
        ExtensionPresence::Absent => Err(shortfall(required, found)),
        ExtensionPresence::Installed(version) if version < required => Err(shortfall(required, found)),
        ExtensionPresence::Installed(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::schema_contract::{
        error::SchemaContractError,
        server_requirements::{
            ExtensionPresence,
            ExtensionVersion,
            ExtensionVersionError,
            PostgresVersion,
            REQUIRED_POSTGIS_VERSION,
            REQUIRED_POSTGRES_VERSION,
            REQUIRED_TIMESCALEDB_VERSION,
            check_extension,
            check_postgres,
            server_requirements_statement,
        },
    };
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_requirements_query_casts_the_server_version_and_reads_both_extensions() {
        let sql = server_requirements_statement().to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"current_setting('server_version_num')::integer AS "server_version_num""#),
            "{sql}"
        );
        assert!(sql.contains(r#""pg_extension""#), "{sql}");
        assert!(sql.contains(r#"AS "postgis_version""#) && sql.contains(r#"AS "timescaledb_version""#), "{sql}");
    }

    #[test]
    fn a_server_below_the_nulls_not_distinct_floor_is_refused_naming_both_versions() {
        let found = PostgresVersion::new(140_012);
        match check_postgres(found) {
            Err(SchemaContractError::PostgresBelowMinimum { required, found: reported }) => {
                pretty_assertions::assert_eq!(required, REQUIRED_POSTGRES_VERSION);
                pretty_assertions::assert_eq!(reported, found);
            }
            Err(other) => panic!("unexpected refusal: {other}"),
            Ok(()) => panic!("PostgreSQL 14.12 is below the floor"),
        }
    }

    #[test]
    fn a_server_at_or_above_the_floor_is_accepted() {
        assert!(check_postgres(PostgresVersion::new(150_000)).is_ok());
        assert!(check_postgres(PostgresVersion::new(180_001)).is_ok());
    }

    #[test]
    fn an_absent_postgis_is_refused_the_same_way_as_one_below_the_minimum() {
        let refusals = [ExtensionPresence::Absent, ExtensionPresence::Installed(ExtensionVersion::new(2, 5))];
        for found in refusals {
            let outcome = check_extension(REQUIRED_POSTGIS_VERSION, found, |required, found| SchemaContractError::PostGisBelowMinimum {
                required,
                found,
            });
            match outcome {
                Err(SchemaContractError::PostGisBelowMinimum { required, found: reported }) => {
                    pretty_assertions::assert_eq!(required, REQUIRED_POSTGIS_VERSION);
                    pretty_assertions::assert_eq!(reported, found);
                }
                Err(other) => panic!("unexpected refusal: {other}"),
                Ok(()) => panic!("{found} does not meet the PostGIS minimum"),
            }
        }
    }

    #[test]
    fn a_timescaledb_below_the_columnstore_floor_is_refused_naming_both_versions() {
        let found = ExtensionPresence::Installed(ExtensionVersion::new(2, 17));
        let outcome = check_extension(REQUIRED_TIMESCALEDB_VERSION, found, |required, found| {
            SchemaContractError::TimescaleDbBelowMinimum { required, found }
        });
        match outcome {
            Err(SchemaContractError::TimescaleDbBelowMinimum { required, found: reported }) => {
                pretty_assertions::assert_eq!(required, REQUIRED_TIMESCALEDB_VERSION);
                pretty_assertions::assert_eq!(reported, found);
            }
            Err(other) => panic!("unexpected refusal: {other}"),
            Ok(()) => panic!("TimescaleDB 2.17 is below the floor"),
        }
    }

    #[test]
    fn an_installed_extension_at_or_above_its_floor_is_accepted() {
        let accepted = [ExtensionVersion::new(3, 0), ExtensionVersion::new(3, 4), ExtensionVersion::new(4, 0)];
        for version in accepted {
            let outcome = check_extension(REQUIRED_POSTGIS_VERSION, ExtensionPresence::Installed(version), |required, found| {
                SchemaContractError::PostGisBelowMinimum { required, found }
            });
            assert!(outcome.is_ok(), "PostGIS {version} meets the minimum");
        }
    }

    #[test]
    fn an_extension_version_reads_its_major_and_minor_including_a_development_suffix() {
        let readable = [
            ("3.4.2", ExtensionVersion::new(3, 4)),
            ("2.17.2", ExtensionVersion::new(2, 17)),
            ("2.18-dev", ExtensionVersion::new(2, 18)),
        ];
        for (raw, expected) in readable {
            match ExtensionVersion::try_from(raw) {
                Ok(version) => pretty_assertions::assert_eq!(version, expected),
                Err(error) => panic!("{raw} is a readable extension version: {error}"),
            }
        }
    }

    #[test]
    fn an_unreadable_extension_version_is_refused_rather_than_assumed_to_be_zero() {
        for raw in ["", "3", "x.y", "3.x"] {
            assert!(
                matches!(ExtensionVersion::try_from(raw), Err(ExtensionVersionError::Unreadable { .. })),
                "{raw:?} must be refused"
            );
        }
    }

    #[test]
    fn an_uninstalled_extension_reads_as_absent_rather_than_as_a_version() {
        match ExtensionPresence::try_from(None::<String>) {
            Ok(presence) => pretty_assertions::assert_eq!(presence, ExtensionPresence::Absent),
            Err(error) => panic!("an absent extension is not an error to read: {error}"),
        }
        pretty_assertions::assert_eq!(ExtensionPresence::Absent.to_string(), "absent");
    }
}

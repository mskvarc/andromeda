//! The server and extension versions the tenant schema depends on.

use crate::schema_contract::{error::SchemaContractError, server_encoding::ServerEncoding};
use andromeda_postgresql_seaquery::{
    cast::cast,
    introspection::{current_setting, extension_version},
    pg_type::PostgresType,
};
use sea_query::{Alias, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Row};
use std::{num::ParseIntError, str::FromStr};

/// The PostgreSQL version the tenant schema requires.
///
/// `temporal_attribute_streams` identifies a multi-attribute instance with
/// `UNIQUE NULLS NOT DISTINCT (temporal_entity_key, attribute_name_key, dataset_id)`,
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
/// core is fully supported: the storage-policy reconciler enables the
/// columnstore only behind a `pg_proc` capability probe, and on the core the
/// hypertables run uncompressed, so no edition is refused here. The columnstore
/// is a TSL benefit that rides along where present, not a requirement.
pub const REQUIRED_TIMESCALEDB_VERSION: ExtensionVersion = ExtensionVersion::new(2, 18);

/// The `server_encoding` the tenant schema requires.
///
/// Every byte bound the migrations state -- the identifier bound on each
/// identifier column, the String overflow bound on `current_attribute_values`
/// -- is an `octet_length` in the server encoding, and the broker measures the
/// same values as UTF-8 octets before it writes them. The two measures agree
/// only when the server encoding is UTF-8; under another encoding a value the
/// broker admitted could violate a check, or land in a column its placement
/// did not choose.
const REQUIRED_SERVER_ENCODING: ServerEncoding = ServerEncoding::Utf8;

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
        let mut components = value.split('.');
        let (Some(major), Some(minor)) = (components.next(), components.next()) else {
            return Err(ExtensionVersionError::MissingMinor);
        };
        Ok(ExtensionVersion {
            major: leading_number(major)?,
            minor: leading_number(minor)?,
        })
    }
}

/// The number the leading ASCII-digit run of a version component spells.
///
/// A component with no leading digit leaves an empty run, which does not parse.
fn leading_number(component: &str) -> Result<u32, ParseIntError> {
    component
        .split(|character: char| !character.is_ascii_digit())
        .next()
        .unwrap_or_default()
        .parse()
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
    /// The reported version has no `.` separating a minor component.
    #[error("an extension version has no minor component")]
    MissingMinor,
    /// A `major.minor` component does not open with a number.
    #[error("an extension version component is not a number: {0}")]
    Component(#[from] ParseIntError),
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
/// fails or a column it returns does not decode,
/// [`SchemaContractError::ExtensionVersionValue`] when a reported
/// extension version cannot be read,
/// [`SchemaContractError::ServerEncodingNotUtf8`] when the server encoding is
/// not UTF-8, and [`SchemaContractError::PostgresBelowMinimum`],
/// [`SchemaContractError::PostGisBelowMinimum`] or
/// [`SchemaContractError::TimescaleDbBelowMinimum`] when the server or one of
/// the two versioned extensions is below the floor the schema depends on.
pub async fn verify_server_requirements(pool: &PgPool) -> Result<(), SchemaContractError> {
    let (sql, values) = server_requirements_statement().build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(pool).await?;

    check_postgres(PostgresVersion::new(row.try_get("server_version_num")?))?;
    check_server_encoding(row.try_get::<&str, _>("server_encoding")?)?;
    check_extension(
        REQUIRED_POSTGIS_VERSION,
        ExtensionPresence::try_from(row.try_get::<Option<String>, _>("postgis_version")?)?,
        |required, found| SchemaContractError::PostGisBelowMinimum { required, found },
    )?;
    check_extension(
        REQUIRED_TIMESCALEDB_VERSION,
        ExtensionPresence::try_from(row.try_get::<Option<String>, _>("timescaledb_version")?)?,
        |required, found| SchemaContractError::TimescaleDbBelowMinimum { required, found },
    )
}

/// Read the server's `server_version_num` and `server_encoding` and the
/// `PostGIS` and `TimescaleDB` extension versions.
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
        .expr_as(current_setting("server_encoding"), Alias::new("server_encoding"))
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

/// Refuse a server whose encoding is not UTF-8.
fn check_server_encoding(reported: &str) -> Result<(), SchemaContractError> {
    let found = ServerEncoding::from_str(reported).map_err(SchemaContractError::ServerEncodingUnreadable)?;
    if found == REQUIRED_SERVER_ENCODING {
        return Ok(());
    }
    Err(SchemaContractError::ServerEncodingNotUtf8 { found })
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
        server_encoding::ServerEncoding,
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
            check_server_encoding,
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
        assert!(sql.contains(r#"current_setting('server_encoding') AS "server_encoding""#), "{sql}");
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

    /// Every byte bound the migrations state is an `octet_length` the broker
    /// measures in UTF-8 octets, which only a UTF-8 server agrees with.
    #[test]
    fn a_server_whose_encoding_is_not_utf8_is_refused_naming_it() {
        assert!(check_server_encoding("UTF8").is_ok());
        match check_server_encoding("LATIN1") {
            Err(SchemaContractError::ServerEncodingNotUtf8 { found }) => pretty_assertions::assert_eq!(found, ServerEncoding::Latin1),
            Err(other) => panic!("unexpected refusal: {other}"),
            Ok(()) => panic!("a LATIN1 server measures octets differently"),
        }
    }

    #[test]
    fn a_server_reporting_an_unknown_encoding_name_is_refused_as_unreadable() {
        assert!(matches!(
            check_server_encoding("KLINGON"),
            Err(SchemaContractError::ServerEncodingUnreadable(_))
        ));
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
        for raw in ["x.y", "3.x", ".4"] {
            assert!(
                matches!(ExtensionVersion::try_from(raw), Err(ExtensionVersionError::Component(_))),
                "{raw:?} must be refused"
            );
        }
    }

    #[test]
    fn an_extension_version_without_a_minor_component_is_refused() {
        for raw in ["", "3"] {
            assert!(
                matches!(ExtensionVersion::try_from(raw), Err(ExtensionVersionError::MissingMinor)),
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

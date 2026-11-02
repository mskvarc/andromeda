//! The server and extension versions the tenant schema depends on.

use crate::schema_contract::{error::SchemaContractError, server_encoding::ServerEncoding, versioned_extension::VersionedExtension};
use andromeda_postgresql_seaquery::{
    introspection::{current_setting, extension_version, server_version_num},
    setting::Setting,
    statement::Statement,
};
use sea_query::{Alias, Query, SelectStatement};
use sqlx::{Executor, Postgres, Row, postgres::PgRow};
use std::{num::ParseIntError, str::FromStr};

/// The PostgreSQL version the tenant schema requires.
///
/// The maintenance catalogue keys its targets with
/// `UNIQUE NULLS NOT DISTINCT (tenant_key, snapshot_key, target)`, where an
/// absent Snapshot is the live branch, and `NULLS NOT DISTINCT` is a
/// PostgreSQL 15 feature.
pub const REQUIRED_POSTGRES_VERSION: PostgresVersion = PostgresVersion::new(150_000);

/// The `PostGIS` version the tenant schema requires.
///
/// The schema stores `geometry(PointZ, 4326)` and casts to `geography` for
/// distance predicates, and the compiled `georel` predicates call
/// `ST_MakeValid`; all of that is `PostGIS` 3.0 surface.
pub const REQUIRED_POSTGIS_VERSION: ExtensionVersion = ExtensionVersion::new(3, 0, 0);

/// The `TimescaleDB` version the tenant schema requires.
///
/// The floor is the release the PostgreSQL image ships, patch included, so a
/// tenant runs the version the broker was built and tested against and the
/// broker carries no path conditional on an older one. A tenant created on an
/// earlier image is brought up to the shipped version by
/// [`update_extensions`](crate::extension_update::update_extensions) before
/// this floor is checked. Both editions ship the same package version, so one
/// floor covers both.
///
/// The floor is a version floor only, never a license floor. The Apache-2.0
/// core is fully supported: the storage-policy reconciler enables the
/// columnstore only behind a `pg_proc` capability probe, and on the core the
/// hypertables run uncompressed, so no edition is refused here. The columnstore
/// is a TSL benefit that rides along where present, not a requirement.
pub const REQUIRED_TIMESCALEDB_VERSION: ExtensionVersion = ExtensionVersion::new(2, 31, 0);

/// The `server_encoding` the tenant schema requires.
///
/// Every byte bound the migrations state (the identifier bound on each
/// identifier column, the String overflow bound on `current_attributes`) is
/// an `octet_length` in the server encoding, and the broker measures the
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

/// The `major.minor.patch` numbers of an extension version.
///
/// Field order is the comparison order: `Ord` is derived, so major dominates,
/// then minor, then patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display)]
#[display("{major}.{minor}.{patch}")]
pub struct ExtensionVersion {
    /// The major component, compared first.
    pub major: u32,
    /// The minor component, compared second.
    pub minor: u32,
    /// The patch component, compared last.
    pub patch: u32,
}

impl ExtensionVersion {
    /// Name a `major.minor.patch` version.
    #[must_use]
    pub const fn new(major: u32, minor: u32, patch: u32) -> ExtensionVersion {
        ExtensionVersion { major, minor, patch }
    }
}

impl TryFrom<&str> for ExtensionVersion {
    type Error = ExtensionVersionError;

    /// Read what `pg_extension.extversion` or
    /// `pg_available_extensions.default_version` holds.
    ///
    /// Only the leading digit run of each component is read, because a
    /// development build reports a component such as `0-dev`; a component with
    /// no leading digit is refused rather than assumed to be zero, since a
    /// version that cannot be read cannot be shown to meet a floor. A version
    /// of two components (`3.0`) is the first release of that minor line, so
    /// its patch is zero.
    fn try_from(value: &str) -> Result<ExtensionVersion, ExtensionVersionError> {
        let mut components = value.split('.');
        let (Some(major), Some(minor)) = (components.next(), components.next()) else {
            return Err(ExtensionVersionError::MissingMinor);
        };
        Ok(ExtensionVersion {
            major: leading_number(major)?,
            minor: leading_number(minor)?,
            patch: components.next().map(leading_number).transpose()?.unwrap_or_default(),
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
/// the schema also creates (`pg_trgm`, `ltree`, `btree_gist`, and `btree_gin`
/// for the `int4_ops` half of the attribute-aware trigram index on
/// `current_attributes`) ship with every supported server, so their
/// `CREATE EXTENSION IF NOT EXISTS` in the migrations is the check.
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
pub async fn verify_server_requirements<'e, E>(executor: E) -> Result<(), SchemaContractError>
where
    E: Executor<'e, Database = Postgres>,
{
    let row = server_requirements_statement().to_query().fetch_one(executor).await?;

    check_server(&row)?;
    check_extension(
        REQUIRED_POSTGIS_VERSION,
        ExtensionPresence::try_from(row.try_get::<Option<String>, _>(VersionedExtension::PostGis.installed_column())?)?,
        |required, found| SchemaContractError::PostGisBelowMinimum { required, found },
    )?;
    check_extension(
        REQUIRED_TIMESCALEDB_VERSION,
        ExtensionPresence::try_from(row.try_get::<Option<String>, _>(VersionedExtension::TimescaleDb.installed_column())?)?,
        |required, found| SchemaContractError::TimescaleDbBelowMinimum { required, found },
    )
}

/// Read the server's `server_version_num` and `server_encoding`, the columns
/// [`check_server`] reads.
///
/// Both columns are system-function calls, so the statement names no
/// application relation and binds the setting names it reads.
pub(crate) fn server_settings_statement() -> SelectStatement {
    Query::select()
        .expr_as(server_version_num(), Alias::new(SERVER_VERSION_NUM_COLUMN))
        .expr_as(current_setting(Setting::ServerEncoding), Alias::new(SERVER_ENCODING_COLUMN))
        .to_owned()
}

/// The column [`server_settings_statement`] reads `server_version_num` under.
const SERVER_VERSION_NUM_COLUMN: &str = "server_version_num";

/// The column [`server_settings_statement`] reads `server_encoding` under.
const SERVER_ENCODING_COLUMN: &str = "server_encoding";

/// Refuse a server below the version floor or whose encoding is not UTF-8,
/// from a row [`server_settings_statement`] projects.
pub(crate) fn check_server(row: &PgRow) -> Result<(), SchemaContractError> {
    check_postgres(PostgresVersion::new(row.try_get(SERVER_VERSION_NUM_COLUMN)?))?;
    check_server_encoding(row.try_get::<&str, _>(SERVER_ENCODING_COLUMN)?)
}

/// Read the server's `server_version_num` and `server_encoding` and the
/// `PostGIS` and `TimescaleDB` extension versions.
///
/// Every column is a system-function call or a `pg_extension` lookup, so the
/// statement names no application relation and binds the setting and extension
/// names it reads.
fn server_requirements_statement() -> SelectStatement {
    server_settings_statement()
        .expr_as(
            extension_version(VersionedExtension::PostGis.catalog_name()),
            Alias::new(VersionedExtension::PostGis.installed_column()),
        )
        .expr_as(
            extension_version(VersionedExtension::TimescaleDb.catalog_name()),
            Alias::new(VersionedExtension::TimescaleDb.installed_column()),
        )
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
#[expect(
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
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
    use andromeda_postgresql_schema::tenancy::EXTENSION_VERSION_BYTE_LIMIT;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_longest_extension_version_fits_the_stored_version_bound() {
        let longest = ExtensionVersion::new(u32::MAX, u32::MAX, u32::MAX).to_string();
        assert!(longest.len() <= EXTENSION_VERSION_BYTE_LIMIT, "{longest}");
    }

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
        let refusals = [ExtensionPresence::Absent, ExtensionPresence::Installed(ExtensionVersion::new(2, 5, 0))];
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
    fn the_timescaledb_floor_is_the_release_the_image_ships() {
        pretty_assertions::assert_eq!(REQUIRED_TIMESCALEDB_VERSION, ExtensionVersion::new(2, 31, 0));
    }

    #[test]
    fn a_timescaledb_below_the_floor_is_refused_naming_both_versions_down_to_the_previous_release() {
        let refusals = [
            ExtensionVersion::new(2, 30, 2),
            ExtensionVersion::new(2, 29, 2),
            ExtensionVersion::new(2, 18, 0),
            ExtensionVersion::new(1, 99, 99),
        ];
        for version in refusals {
            let found = ExtensionPresence::Installed(version);
            let outcome = check_extension(REQUIRED_TIMESCALEDB_VERSION, found, |required, found| {
                SchemaContractError::TimescaleDbBelowMinimum { required, found }
            });
            match outcome {
                Err(SchemaContractError::TimescaleDbBelowMinimum { required, found: reported }) => {
                    pretty_assertions::assert_eq!(required, REQUIRED_TIMESCALEDB_VERSION);
                    pretty_assertions::assert_eq!(reported, found);
                }
                Err(other) => panic!("unexpected refusal: {other}"),
                Ok(()) => panic!("TimescaleDB {version} is below the floor"),
            }
        }
    }

    #[test]
    fn a_timescaledb_at_the_floor_or_any_later_version_is_accepted() {
        let accepted = [
            ExtensionVersion::new(2, 31, 0),
            ExtensionVersion::new(2, 31, 1),
            ExtensionVersion::new(2, 32, 0),
            ExtensionVersion::new(3, 0, 0),
        ];
        for version in accepted {
            let outcome = check_extension(REQUIRED_TIMESCALEDB_VERSION, ExtensionPresence::Installed(version), |required, found| {
                SchemaContractError::TimescaleDbBelowMinimum { required, found }
            });
            assert!(outcome.is_ok(), "TimescaleDB {version} meets the floor");
        }
    }

    #[test]
    fn an_installed_extension_at_or_above_its_floor_is_accepted() {
        let accepted = [ExtensionVersion::new(3, 0, 0), ExtensionVersion::new(3, 4, 2), ExtensionVersion::new(4, 0, 0)];
        for version in accepted {
            let outcome = check_extension(REQUIRED_POSTGIS_VERSION, ExtensionPresence::Installed(version), |required, found| {
                SchemaContractError::PostGisBelowMinimum { required, found }
            });
            assert!(outcome.is_ok(), "PostGIS {version} meets the minimum");
        }
    }

    #[test]
    fn an_extension_version_reads_its_three_components_including_a_development_suffix() {
        let readable = [
            ("3.6.4", ExtensionVersion::new(3, 6, 4)),
            ("2.30.2", ExtensionVersion::new(2, 30, 2)),
            ("2.31.0-dev", ExtensionVersion::new(2, 31, 0)),
            ("2.18-dev", ExtensionVersion::new(2, 18, 0)),
            ("3.0", ExtensionVersion::new(3, 0, 0)),
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
        for raw in ["x.y", "3.x", ".4", "3.6.x"] {
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
    fn an_extension_version_renders_all_three_components() {
        pretty_assertions::assert_eq!(ExtensionVersion::new(2, 30, 2).to_string(), "2.30.2");
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

//! The name of the tenant template, and the start every template name of one
//! prefix shares.
//!
//! A template is named after the prefix tenant databases take, the infix
//! `-template-` and the fingerprint of what it is built against: the first 4
//! bytes of the SHA-256 digest (FIPS 180-4) of the tenant migration set's
//! digest and the version the server ships of every tracked extension, as 8
//! lowercase hexadecimal digits. A broker whose migrations or server
//! extensions differ from a template's expects another name, so it builds a
//! template of its own instead of copying one built against something else.
//! Two fingerprints agree by chance once in 2^32 pairs of builds.
//!
//! The prefix grammar admits no `-`, and a tenant database name joins the
//! prefix to its label with `_`. No tenant database is ever named like a
//! template, no template of one prefix starts like another prefix's, and the
//! maintenance check that reports an unregistered `<prefix>_` database as an
//! orphan never reports a template.

use crate::{extension_standing::ExtensionStanding, schema_contract::contract::SchemaContract};
use andromeda_configuration::schema::db_name_prefix::DbNamePrefix;
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use sha2::{Digest, Sha256};

/// What joins the prefix to the fingerprint.
const TEMPLATE_INFIX: &str = "-template-";

/// The bytes of the digest the fingerprint spells: 32 bits.
const FINGERPRINT_BYTES: usize = 4;

/// The name of the template built against `contract` on a server whose tracked
/// extensions stand as `standings` say, for tenant databases named after
/// `prefix`.
///
/// The name fits PostgreSQL's 63 bytes for every prefix the configuration
/// admits.
pub(crate) fn template_database_name(prefix: &DbNamePrefix, contract: &SchemaContract, standings: &[ExtensionStanding]) -> TenantDatabaseName {
    TenantDatabaseName::from(format!("{}{}", family_start(prefix), fingerprint(contract, standings)))
}

/// What every template name of `prefix` starts with.
pub(crate) fn family_start(prefix: &DbNamePrefix) -> String {
    format!("{prefix}{TEMPLATE_INFIX}")
}

/// The fingerprint of the migration set `contract` digests and the shipped
/// version of every extension in `standings`, in hexadecimal.
fn fingerprint(contract: &SchemaContract, standings: &[ExtensionStanding]) -> String {
    let digest = standings
        .iter()
        .fold(Sha256::new_with_prefix(contract.digest().to_string()), |mut digest, standing| {
            digest.update(shipped_entry(standing));
            digest
        });
    hex::encode(&digest.finalize()[..FINGERPRINT_BYTES])
}

/// One extension's line of the fingerprint's input: its name and the version
/// the server ships, empty when it ships none.
fn shipped_entry(standing: &ExtensionStanding) -> String {
    let name = standing.extension.catalog_name();
    match standing.shipped {
        Some(version) => format!("\n{name}={version}"),
        None => format!("\n{name}="),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        extension_standing::ExtensionStanding,
        recreated_extension::RecreatedExtension,
        schema_contract::{
            contract::SchemaContract,
            server_requirements::{ExtensionPresence, ExtensionVersion},
            versioned_extension::VersionedExtension,
        },
        tenant_database_naming::derive_database_name,
        tenant_template::template_name::{family_start, template_database_name},
        tracked_extension::TrackedExtension,
    };
    use andromeda_configuration::schema::db_name_prefix::{DB_NAME_PREFIX_BYTE_LIMIT, DbNamePrefix};
    use andromeda_ngsi_ld_lexicon::tenant::TenantName;
    use andromeda_postgresql_schema::tenancy::DATABASE_NAME_BYTE_LIMIT;
    use pretty_assertions::{assert_eq, assert_ne};
    use sqlx::migrate::{Migration, MigrationType, Migrator};
    use std::borrow::Cow;

    /// The contract of a one-migration set whose migration runs `sql`.
    fn contract(sql: &'static str) -> SchemaContract {
        let migrator = Migrator {
            migrations: Cow::Owned(vec![Migration::new(
                1,
                Cow::Borrowed("entities"),
                MigrationType::ReversibleUp,
                sqlx::SqlStr::from_static(sql),
                false,
            )]),
            ..Migrator::DEFAULT
        };
        SchemaContract::of(&migrator).expect("the fixture set has a contract")
    }

    /// A server shipping `TimescaleDB` at `timescaledb` and `PostGIS` and
    /// `pg_repack` at fixed versions.
    fn standings(timescaledb: ExtensionVersion) -> Vec<ExtensionStanding> {
        let shipped = |extension, version| ExtensionStanding {
            extension,
            installed: ExtensionPresence::Absent,
            shipped: Some(version),
        };
        vec![
            shipped(TrackedExtension::Updated(VersionedExtension::PostGis), ExtensionVersion::new(3, 6, 4)),
            shipped(TrackedExtension::Updated(VersionedExtension::TimescaleDb), timescaledb),
            shipped(TrackedExtension::Recreated(RecreatedExtension::PgRepack), ExtensionVersion::new(1, 5, 3)),
        ]
    }

    #[test]
    fn a_template_is_named_after_the_prefix_and_eight_hexadecimal_digits_of_fingerprint() {
        let name = template_database_name(
            &DbNamePrefix::default(),
            &contract("CREATE TABLE entities ()"),
            &standings(ExtensionVersion::new(2, 31, 0)),
        );
        let fingerprint = name.as_str().strip_prefix("andromeda_tenant-template-").expect("the family start");
        assert_eq!(fingerprint.len(), 8, "{name}");
        assert!(
            fingerprint.chars().all(|digit| digit.is_ascii_hexdigit() && !digit.is_ascii_uppercase()),
            "{name}"
        );
    }

    #[test]
    fn the_same_migrations_and_shipped_extensions_name_the_same_template() {
        let first = template_database_name(
            &DbNamePrefix::default(),
            &contract("CREATE TABLE entities ()"),
            &standings(ExtensionVersion::new(2, 31, 0)),
        );
        let second = template_database_name(
            &DbNamePrefix::default(),
            &contract("CREATE TABLE entities ()"),
            &standings(ExtensionVersion::new(2, 31, 0)),
        );
        assert_eq!(first, second);
    }

    #[test]
    fn other_migrations_or_another_shipped_extension_version_name_another_template() {
        let built = template_database_name(
            &DbNamePrefix::default(),
            &contract("CREATE TABLE entities ()"),
            &standings(ExtensionVersion::new(2, 31, 0)),
        );
        let migrated = template_database_name(
            &DbNamePrefix::default(),
            &contract("CREATE TABLE entities (id UUID)"),
            &standings(ExtensionVersion::new(2, 31, 0)),
        );
        let upgraded = template_database_name(
            &DbNamePrefix::default(),
            &contract("CREATE TABLE entities ()"),
            &standings(ExtensionVersion::new(2, 31, 1)),
        );
        assert_ne!(built, migrated);
        assert_ne!(built, upgraded);
    }

    #[test]
    fn an_extension_the_server_stops_shipping_names_another_template() {
        let shipping = standings(ExtensionVersion::new(2, 31, 0));
        let mut withdrawn = shipping.clone();
        if let Some(pg_repack) = withdrawn.last_mut() {
            pg_repack.shipped = None;
        }
        let contract = contract("CREATE TABLE entities ()");
        assert_ne!(
            template_database_name(&DbNamePrefix::default(), &contract, &shipping),
            template_database_name(&DbNamePrefix::default(), &contract, &withdrawn)
        );
    }

    #[test]
    fn a_template_name_fits_the_identifier_limit_at_the_longest_prefix() {
        let prefix = DbNamePrefix::try_from("p".repeat(DB_NAME_PREFIX_BYTE_LIMIT)).expect("a prefix at the limit");
        let name = template_database_name(&prefix, &contract("CREATE TABLE entities ()"), &standings(ExtensionVersion::new(2, 31, 0)));
        assert!(name.as_str().len() <= DATABASE_NAME_BYTE_LIMIT, "{} bytes: {name}", name.as_str().len());
    }

    #[test]
    fn no_tenant_database_is_named_like_a_template() {
        let prefix = DbNamePrefix::default();
        for tenant in ["template", "Template", "template_", "a"] {
            let tenant_database = derive_database_name(&prefix, &tenant.parse::<TenantName>().expect("a tenant name"));
            assert!(!tenant_database.as_str().starts_with(&family_start(&prefix)), "{tenant_database}");
        }
        let template = template_database_name(&prefix, &contract("CREATE TABLE entities ()"), &standings(ExtensionVersion::new(2, 31, 0)));
        assert!(!template.as_str().starts_with("andromeda_tenant_"), "{template}");
    }

    #[test]
    fn no_template_of_one_prefix_starts_like_another_prefixs() {
        let short = DbNamePrefix::try_from("andromeda").expect("a prefix");
        let long = DbNamePrefix::try_from("andromeda_tenant").expect("a prefix");
        assert!(!family_start(&long).starts_with(&family_start(&short)));
        assert!(!family_start(&short).starts_with(&family_start(&long)));
    }
}

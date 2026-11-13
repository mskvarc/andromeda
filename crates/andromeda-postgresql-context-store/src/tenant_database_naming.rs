//! The name of the database a named tenant's data lives in.
//!
//! A tenant's database is named once, when the tenant is first registered, and
//! the registry keeps that name. Every later reader takes the stored name, so a
//! name once given never changes, whatever this derivation would give.
//!
//! The name is the configured prefix, an underscore, the tenant's label, an
//! underscore and the digest suffix. The label is the tenant name's runs of
//! ASCII letters and digits, lowercased and joined by underscores, cut to the
//! room PostgreSQL's 63-byte limit leaves, and `tenant` for a name holding no
//! letter or digit. It is there for an operator reading the server's database
//! list. Many tenant names share a label (`Acme`, `acme` and `acme_`, or two
//! long names cut at the same length), so the suffix is what tells them apart:
//! the first 8 bytes of the SHA-256 digest (FIPS 180-4) of the tenant name's
//! UTF-8 octets, as 16 lowercase hexadecimal digits. Its standard fixes the
//! digest, so a tenant name derives the same database name on every build, and
//! two tenant names derive the same one only when their labels match and so do
//! the first 64 bits of their digests.

use andromeda_configuration::schema::db_name_prefix::DbNamePrefix;
use andromeda_ngsi_ld_lexicon::{tenant::TenantName, tenant_database_name::TenantDatabaseName};
use andromeda_postgresql_schema::tenancy::DATABASE_NAME_BYTE_LIMIT;
use sha2::{Digest, Sha256};

/// The bytes of the tenant name's SHA-256 digest the suffix spells: 64 bits.
const SUFFIX_DIGEST_BYTES: usize = 8;

/// The characters of the suffix, two hexadecimal digits per digest byte.
const SUFFIX_LENGTH: usize = 2 * SUFFIX_DIGEST_BYTES;

/// The label of a tenant name holding no ASCII letter or digit, such as `_`.
const PLACEHOLDER_LABEL: &str = "tenant";

/// The name of the database a tenant registered under `tenant_name` gets, with
/// `prefix` ahead of it.
///
/// The name fits [`DATABASE_NAME_BYTE_LIMIT`] for every prefix the
/// configuration admits, which leaves at least one byte of label.
#[must_use]
pub fn derive_database_name(prefix: &DbNamePrefix, tenant_name: &TenantName) -> TenantDatabaseName {
    let prefix = prefix.as_str();
    let room = DATABASE_NAME_BYTE_LIMIT.saturating_sub(prefix.len() + 1 + 1 + SUFFIX_LENGTH);
    // The label is ASCII, so a character is a byte.
    let cut: String = label(tenant_name).chars().take(room).collect();
    let label = cut.trim_end_matches('_');
    TenantDatabaseName::from(format!("{prefix}_{label}_{}", suffix(tenant_name)))
}

/// The tenant name's runs of ASCII letters and digits, lowercased and joined by
/// underscores, or [`PLACEHOLDER_LABEL`] when it holds none.
fn label(tenant_name: &TenantName) -> String {
    let joined = tenant_name
        .as_str()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|run| !run.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join("_");
    if joined.is_empty() { PLACEHOLDER_LABEL.to_owned() } else { joined }
}

/// The first [`SUFFIX_DIGEST_BYTES`] of the SHA-256 digest of the tenant name,
/// in lowercase hexadecimal.
fn suffix(tenant_name: &TenantName) -> String {
    let digest = Sha256::digest(tenant_name.as_str().as_bytes());
    hex::encode(&digest[..SUFFIX_DIGEST_BYTES])
}

#[cfg(test)]
mod tests {
    use crate::tenant_database_naming::derive_database_name;
    use andromeda_configuration::schema::db_name_prefix::{DB_NAME_PREFIX_BYTE_LIMIT, DbNamePrefix};
    use andromeda_ngsi_ld_lexicon::tenant::{TENANT_NAME_BYTE_LIMIT, TenantName};
    use andromeda_postgresql_schema::tenancy::DATABASE_NAME_BYTE_LIMIT;
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;

    /// The name `tenant_name` derives under the default prefix.
    fn derived(tenant_name: &str) -> String {
        let tenant_name = tenant_name.parse::<TenantName>().expect("a tenant name");
        derive_database_name(&DbNamePrefix::default(), &tenant_name).as_str().to_owned()
    }

    /// The suffix is the first 8 bytes of SHA-256("abc"), the digest FIPS 180-4
    /// gives as its first example: ba7816bf8f01cfea414140de5dae2223...
    #[test]
    fn the_suffix_is_the_first_eight_bytes_of_the_sha256_digest_of_the_tenant_name() {
        assert_eq!(derived("abc"), "andromeda_tenant_abc_ba7816bf8f01cfea");
    }

    /// The recorded names hold on every build: a change of toolchain or
    /// dependency that moved one would fail here.
    #[test]
    fn a_tenant_name_derives_the_recorded_database_name() {
        let recorded = [
            ("acme", "andromeda_tenant_acme_822b33ad87c148a0"),
            ("Acme", "andromeda_tenant_acme_37036cd8f9746d33"),
            ("acme_", "andromeda_tenant_acme_01c375d5da75ff2f"),
            ("smart-city", "andromeda_tenant_smart_city_fc0f9b949ad9484f"),
            ("TEnAnTLABelProbEcase", "andromeda_tenant_tenantlabelprobecase_4b3fcffe8fb4178d"),
            ("TenANTlaBELPRobEcase", "andromeda_tenant_tenantlabelprobecase_d9f284bab0a24cff"),
            (
                "tenantlabelprobetruncatedcommonprefixabc-2o6a",
                "andromeda_tenant_tenantlabelprobetruncatedcomm_9f0b7ba4547bfcc4",
            ),
            (
                "tenantlabelprobetruncatedcommonprefixabc-utob",
                "andromeda_tenant_tenantlabelprobetruncatedcomm_363aa8b661c4184d",
            ),
        ];
        for (tenant_name, database_name) in recorded {
            assert_eq!(derived(tenant_name), database_name, "{tenant_name}");
        }
    }

    #[test]
    fn tenant_names_sharing_a_label_derive_distinct_database_names() {
        let sharing = ["acme", "Acme", "ACME", "acme_", "_acme", "a_c_m_e"];
        let names = sharing.iter().map(|tenant_name| derived(tenant_name)).collect::<HashSet<_>>();
        assert_eq!(names.len(), sharing.len(), "{names:?}");
    }

    #[test]
    fn a_tenant_name_holding_no_letter_or_digit_takes_the_placeholder_label() {
        assert_eq!(derived("_"), "andromeda_tenant_tenant_d2e2adf7177b7a8a");
    }

    #[test]
    fn the_longest_tenant_name_is_cut_to_the_room_the_prefix_leaves() {
        let longest = format!("t{}", "a".repeat(TENANT_NAME_BYTE_LIMIT - 1));
        let name = derived(&longest);
        assert_eq!(name, "andromeda_tenant_taaaaaaaaaaaaaaaaaaaaaaaaaaaa_84edbbff808972cc");
        assert_eq!(name.len(), DATABASE_NAME_BYTE_LIMIT);
    }

    #[test]
    fn a_label_cut_just_after_an_underscore_drops_that_underscore() {
        assert_eq!(
            derived("abcdefghijklmnopqrstuvwxyzab-cd"),
            "andromeda_tenant_abcdefghijklmnopqrstuvwxyzab_7b2f8accb86395ba"
        );
    }

    #[test]
    fn the_longest_prefix_leaves_one_byte_of_label_within_the_limit() {
        let prefix = DbNamePrefix::try_from("p".repeat(DB_NAME_PREFIX_BYTE_LIMIT)).expect("a prefix at the limit");
        let tenant_name = "acme".parse::<TenantName>().expect("a tenant name");
        let name = derive_database_name(&prefix, &tenant_name);
        assert_eq!(name.as_str(), format!("{}_a_822b33ad87c148a0", "p".repeat(DB_NAME_PREFIX_BYTE_LIMIT)));
        assert_eq!(name.as_str().len(), DATABASE_NAME_BYTE_LIMIT);
    }
}

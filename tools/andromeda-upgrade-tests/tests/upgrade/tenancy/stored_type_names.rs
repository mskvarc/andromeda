//! A type name v1.0.0 interned that expands to no IRI, as it stored the
//! `objectType` and the `vocab` a client wrote with a `{` or a stray `%`, is
//! read back by this build's type dictionary once the tenant is upgraded, so
//! the tenant still opens.

use crate::fixtures::{rehearsal::at_v1_0_0, statements::insert, test_error::UpgradeTestError};
use andromeda_configuration::schema::database::name_cache::NameCacheConfig;
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_postgresql_name_dictionary::name_cache::NameCache;
use sea_query::Expr;

/// The type v1.0.0 interned for the `objectType` `ex:a{b`, which it expanded
/// against the default vocabulary.
const INTERNED_BRACE: &str = "https://uri.etsi.org/ngsi-ld/default-context/ex:a{b";

/// The type v1.0.0 interned for the `vocab` `ex:%`.
const INTERNED_PERCENT: &str = "https://uri.etsi.org/ngsi-ld/default-context/ex:%";

/// The type v1.0.0 interned for the `objectType` `urn:x:{`, which it kept as
/// written.
const INTERNED_URN_BRACE: &str = "urn:x:{";

/// The type v1.0.0 interned for the `vocab` `https://example.org/{`, which it
/// kept as written.
const INTERNED_URL_BRACE: &str = "https://example.org/{";

#[tokio::test]
async fn a_type_name_v1_0_0_interned_that_is_no_iri_is_read_back_by_the_type_dictionary_after_the_upgrade() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    for name in [INTERNED_BRACE, INTERNED_PERCENT, INTERNED_URN_BRACE, INTERNED_URL_BRACE] {
        insert(database.pool(), "entity_type_names", vec![("name", Expr::val(name))]).await?;
    }

    database.migrate_to_current().await?;

    NameCache::<EntityTypeName>::new(database.pool(), &NameCacheConfig::default()).preload().await?;
    Ok(())
}

//! Name cache eviction: a name the bounded cache has dropped still resolves.
//!
//! Each dictionary's in-process cache holds only what its byte budget allows,
//! and the dictionary table behind it is the source of truth. A cache far
//! smaller than the dictionary must therefore answer every name with the key
//! the table holds for it, on the first resolution and again after the names
//! have pushed one another out, and a preload must stop at what the budget
//! holds instead of failing on a dictionary larger than it.

use crate::GENERATED_NAME_PREFIX;
use andromeda_configuration_scalars::human_byte_size::HumanByteSize;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_postgresql_name_dictionary::{
    name_cache::NameCache,
    name_cache_config::{IdleEviction, NameCacheConfig, Unlimited},
    name_lookup::NameLookup,
};
use andromeda_test_harness::{configuration::TestRunConfiguration, database_access::connect_test_database, serialized_environment::SerializedEnvironment};
use pretty_assertions::assert_eq;
use sqlx::{PgPool, Row};
use std::collections::HashMap;

/// Names interned for the check, far more than the cache below can hold.
const GENERATED_NAMES: i32 = 2_000;

/// A budget of a few dozen entries, so the generated names evict one another.
const TINY_BUDGET: HumanByteSize = HumanByteSize::kib(8);

/// Remove exactly the names this check interned.
async fn remove_generated_names(pool: &PgPool) {
    sqlx::query("DELETE FROM attribute_names WHERE name LIKE $1")
        .bind(format!("{GENERATED_NAME_PREFIX}%"))
        .execute(pool)
        .await
        .expect("the generated dictionary entries must be removable");
}

/// The key the dictionary table holds for every generated name.
async fn stored_keys(pool: &PgPool) -> HashMap<String, i32> {
    sqlx::query("SELECT name, attribute_name_key FROM attribute_names WHERE name LIKE $1")
        .bind(format!("{GENERATED_NAME_PREFIX}%"))
        .fetch_all(pool)
        .await
        .expect("the dictionary must be readable")
        .iter()
        .map(|row| (row.get("name"), row.get("attribute_name_key")))
        .collect()
}

/// Resolve every generated name through `cache` and report each one's key.
async fn resolved_keys(cache: &NameCache<AttributeName>, names: &[AttributeName]) -> HashMap<String, i32> {
    let resolved = cache.lookup(names).await.expect("resolving names already stored must not fail");
    names
        .iter()
        .filter_map(|name| resolved.get(name).map(|key| (name.to_string(), key.get())))
        .collect()
}

#[tokio::test]
async fn names_evicted_from_a_tiny_cache_still_resolve_to_the_keys_the_dictionary_holds() {
    let configuration = TestRunConfiguration::from_environment().expect("integration test configuration");
    let environment = SerializedEnvironment::acquire(&configuration)
        .await
        .expect("exclusive ownership of the shared dictionary");
    let pool = connect_test_database().await.expect("test database connection");

    remove_generated_names(&pool).await;
    sqlx::query("INSERT INTO attribute_names (name) SELECT $1 || generated::TEXT FROM generate_series(1, $2) AS generated ON CONFLICT (name) DO NOTHING")
        .bind(GENERATED_NAME_PREFIX)
        .bind(GENERATED_NAMES)
        .execute(&pool)
        .await
        .expect("interning distinct attribute names must not fail");
    let stored = stored_keys(&pool).await;
    let names: Vec<AttributeName> = stored.keys().map(|name| name.parse().expect("a stored name is valid")).collect();

    let cache = NameCache::<AttributeName>::new(
        &pool,
        &NameCacheConfig {
            capacity_per_dictionary: TINY_BUDGET,
            idle_eviction: IdleEviction::Never(Unlimited::Unlimited),
        },
    );
    let preloaded = cache.preload().await;
    let first_pass = resolved_keys(&cache, &names).await;
    let second_pass = resolved_keys(&cache, &names).await;

    remove_generated_names(&pool).await;
    pool.close().await;

    assert!(preloaded.is_ok(), "a dictionary larger than the budget preloads what fits: {preloaded:?}");
    assert_eq!(stored.len(), usize::try_from(GENERATED_NAMES).expect("fits"), "one row per generated name");
    assert_eq!(first_pass, stored, "every name resolves to its stored key although the cache holds few of them");
    assert_eq!(second_pass, stored, "names the cache dropped resolve to the same keys again");

    environment.finish().await.expect("releasing exclusive ownership failed");
}

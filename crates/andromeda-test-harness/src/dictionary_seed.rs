//! Seeding the tenant name dictionaries for tests that write rows directly.
//!
//! A test that writes a branch row itself needs the dictionary key of the
//! attribute or entity type IRI the row names. The dictionaries admit an insert
//! and nothing else, so a key is resolved by an insert that does nothing on a
//! name already held, followed by a read of the key; an upsert would take the
//! update path for a held name and be refused. The read is its own statement,
//! so under read committed it sees a row a concurrent insert committed while
//! this one waited on it.

use sqlx::PgPool;

/// The two statements that resolve the key of one name in one dictionary.
struct SeedStatements {
    /// Inserts the bound name unless the dictionary already holds it.
    insert: &'static str,
    /// Reads the key of the bound name.
    select: &'static str,
}

/// The statements of [`attribute_name_key`].
const ATTRIBUTE_NAME: SeedStatements = SeedStatements {
    insert: "INSERT INTO attribute_names (name) VALUES ($1) ON CONFLICT (name) DO NOTHING",
    select: "SELECT attribute_name_key FROM attribute_names WHERE name = $1",
};

/// The statements of [`entity_type_name_key`].
const ENTITY_TYPE_NAME: SeedStatements = SeedStatements {
    insert: "INSERT INTO entity_type_names (name) VALUES ($1) ON CONFLICT (name) DO NOTHING",
    select: "SELECT entity_type_name_key FROM entity_type_names WHERE name = $1",
};

/// A dictionary seed statement failed.
#[derive(Debug, thiserror::Error)]
#[error("seeding a dictionary name failed: {0}")]
pub struct DictionarySeedError(#[from] sqlx::Error);

/// The key `name` holds in `attribute_names`, minted when the dictionary does
/// not hold it yet.
///
/// # Errors
///
/// Returns [`DictionarySeedError`] when either statement fails.
pub async fn attribute_name_key(pool: &PgPool, name: &str) -> Result<i32, DictionarySeedError> {
    dictionary_key(pool, &ATTRIBUTE_NAME, name).await
}

/// The key `name` holds in `entity_type_names`, minted when the dictionary
/// does not hold it yet.
///
/// # Errors
///
/// Returns [`DictionarySeedError`] when either statement fails.
pub async fn entity_type_name_key(pool: &PgPool, name: &str) -> Result<i32, DictionarySeedError> {
    dictionary_key(pool, &ENTITY_TYPE_NAME, name).await
}

/// Runs `statements` for `name` and returns the key read back.
async fn dictionary_key(pool: &PgPool, statements: &SeedStatements, name: &str) -> Result<i32, DictionarySeedError> {
    sqlx::query(statements.insert).bind(name).execute(pool).await?;
    Ok(sqlx::query_scalar(statements.select).bind(name).fetch_one(pool).await?)
}

#[cfg(test)]
mod tests {
    use crate::dictionary_seed::{ATTRIBUTE_NAME, ENTITY_TYPE_NAME};
    use pretty_assertions::assert_eq;

    #[test]
    fn seeding_a_dictionary_key_never_updates_a_dictionary_row() {
        for (statements, table, key) in [
            (&ATTRIBUTE_NAME, "attribute_names", "attribute_name_key"),
            (&ENTITY_TYPE_NAME, "entity_type_names", "entity_type_name_key"),
        ] {
            assert!(statements.insert.starts_with(&format!("INSERT INTO {table} (name) ")), "{}", statements.insert);
            assert!(statements.insert.ends_with("ON CONFLICT (name) DO NOTHING"), "{}", statements.insert);
            assert_eq!(statements.select, format!("SELECT {key} FROM {table} WHERE name = $1"));
            assert!(
                [statements.insert, statements.select]
                    .iter()
                    .all(|statement| !statement.contains("DO UPDATE") && !statement.starts_with("UPDATE")),
                "a dictionary seed never takes an update path"
            );
        }
    }
}

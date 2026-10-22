//! Seeding the tenant name dictionaries for tests that write rows directly.
//!
//! A test that writes a branch row itself needs the dictionary key of the
//! attribute or entity type IRI the row names. The dictionaries admit an insert
//! and nothing else, so a key is resolved by an insert that does nothing on a
//! name already held, followed by a read of the key; an upsert would take the
//! update path for a held name and be refused. The read is its own statement,
//! so under read committed it sees a row a concurrent insert committed while
//! this one waited on it.

use andromeda_postgresql_schema::name_dictionary::{AttributeNames, EntityTypeNames};
use andromeda_postgresql_seaquery::statement::Statement;
use sea_query::{DynIden, Expr, ExprTrait, InsertStatement, IntoIden, OnConflict, Query, SelectStatement, error::Error as SeaQueryError};
use sqlx::PgPool;

/// One name dictionary: its relation and the columns of a name and its key.
struct Dictionary {
    /// The dictionary relation.
    table: DynIden,
    /// The column holding the expanded IRI.
    name: DynIden,
    /// The column holding the compact integer key.
    key: DynIden,
}

impl Dictionary {
    /// The `attribute_names` dictionary.
    fn attribute_names() -> Dictionary {
        Dictionary {
            table: AttributeNames::Table.into_iden(),
            name: AttributeNames::Name.into_iden(),
            key: AttributeNames::AttributeNameKey.into_iden(),
        }
    }

    /// The `entity_type_names` dictionary.
    fn entity_type_names() -> Dictionary {
        Dictionary {
            table: EntityTypeNames::Table.into_iden(),
            name: EntityTypeNames::Name.into_iden(),
            key: EntityTypeNames::EntityTypeNameKey.into_iden(),
        }
    }

    /// Inserts `name` unless the dictionary already holds it.
    fn insert(&self, name: &str) -> Result<InsertStatement, SeaQueryError> {
        Ok(Query::insert()
            .into_table(self.table.clone()) // clone: the statement owns the table it names
            .columns([self.name.clone()]) // clone: the statement owns the column it names
            .values([Expr::val(name)])?
            .on_conflict(OnConflict::column(self.name.clone()).do_nothing().to_owned()) // clone: the conflict target owns the column it names
            .to_owned())
    }

    /// Reads the key of `name`.
    fn select(&self, name: &str) -> SelectStatement {
        Query::select()
            .column(self.key.clone()) // clone: the statement owns the column it reads
            .from(self.table.clone()) // clone: the statement owns the table it reads
            .and_where(Expr::col(self.name.clone()).eq(name)) // clone: the predicate owns the column it compares
            .to_owned()
    }
}

/// A dictionary seed statement failed.
#[derive(Debug, thiserror::Error)]
pub enum DictionarySeedError {
    /// The database refused a statement.
    #[error("seeding a dictionary name failed: {0}")]
    Database(#[from] sqlx::Error),
    /// The insert could not be assembled.
    #[error("the dictionary seed could not be built: {0}")]
    Statement(#[from] SeaQueryError),
}

/// The key `name` holds in `attribute_names`, minted when the dictionary does
/// not hold it yet.
///
/// # Errors
///
/// Returns [`DictionarySeedError`] when either statement fails.
pub async fn attribute_name_key(pool: &PgPool, name: &str) -> Result<i32, DictionarySeedError> {
    dictionary_key(pool, &Dictionary::attribute_names(), name).await
}

/// The key `name` holds in `entity_type_names`, minted when the dictionary
/// does not hold it yet.
///
/// # Errors
///
/// Returns [`DictionarySeedError`] when either statement fails.
pub async fn entity_type_name_key(pool: &PgPool, name: &str) -> Result<i32, DictionarySeedError> {
    dictionary_key(pool, &Dictionary::entity_type_names(), name).await
}

/// Runs the seed of `dictionary` for `name` and returns the key read back.
async fn dictionary_key(pool: &PgPool, dictionary: &Dictionary, name: &str) -> Result<i32, DictionarySeedError> {
    dictionary.insert(name)?.to_query().execute(pool).await?;
    Ok(dictionary.select(name).to_query_scalar().fetch_one(pool).await?)
}

#[cfg(test)]
mod tests {
    use crate::dictionary_seed::Dictionary;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn seeding_a_dictionary_key_never_updates_a_dictionary_row() {
        for (dictionary, table, key) in [
            (Dictionary::attribute_names(), "attribute_names", "attribute_name_key"),
            (Dictionary::entity_type_names(), "entity_type_names", "entity_type_name_key"),
        ] {
            let (insert, values) = dictionary
                .insert("https://example.org/speed")
                .expect("the insert builds")
                .build(PostgresQueryBuilder);
            assert_eq!(insert, format!(r#"INSERT INTO "{table}" ("name") VALUES ($1) ON CONFLICT ("name") DO NOTHING"#));
            assert_eq!(values.0.len(), 1);
            let (select, _) = dictionary.select("https://example.org/speed").build(PostgresQueryBuilder);
            assert_eq!(select, format!(r#"SELECT "{key}" FROM "{table}" WHERE "name" = $1"#));
            assert!(!insert.contains("DO UPDATE"), "a dictionary seed never takes an update path");
        }
    }
}

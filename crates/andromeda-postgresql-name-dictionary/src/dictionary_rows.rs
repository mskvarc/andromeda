//! Reading dictionary result sets back into typed name/key pairs.

use crate::{
    dictionary_relation::{DictionaryKind, DictionaryRelation},
    dictionary_statements::SNAPSHOT_IS_COMMITTED_COLUMN,
    name_lookup::NameLookupError,
};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReferenceError;
use sqlx::{Row, postgres::PgRow};

/// Whether what a read resolved may be published to the process-wide cache.
///
/// This is the difference between the two reads the dictionary performs, and it
/// is a correctness property rather than a tuning knob: a mapping published from
/// inside an uncommitted transaction would answer every later resolution in the
/// process with a key that a rollback then took away, and the rows written
/// against that key would reference a dictionary entry that does not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CachePublication {
    /// The read saw a committed snapshot, so what it resolved is durable and
    /// may answer later resolutions without a round trip.
    Publish,
    /// The read ran inside a caller's transaction and may have seen rows that
    /// transaction has not committed, so what it resolved answers this caller
    /// only.
    Withhold,
}

/// Read one dictionary result set back into typed pairs.
///
/// The row's `name` is the text that was bound, so each hit could be matched
/// back to the name that produced it; it is parsed instead because `preload`
/// reads rows nobody asked for, and one decoding serves both.
pub(crate) fn decode<N: DictionaryRelation>(rows: &[PgRow]) -> Result<Vec<(N, N::Key)>, NameLookupError> {
    let key_column = N::key_column().to_string();
    let name_column = N::name_column().to_string();
    rows.iter()
        .map(|row| {
            let stored: String = row.try_get(name_column.as_str())?;
            let name = stored.parse::<N>().map_err(stored_name_error::<N>)?;
            Ok((name, N::Key::from(row.try_get::<i32, _>(key_column.as_str())?)))
        })
        .collect()
}

/// The failure a stored row that is not a valid name of type `N` becomes.
const fn stored_name_error<N: DictionaryRelation>(source: NgsiLdReferenceError) -> NameLookupError {
    match N::KIND {
        DictionaryKind::AttributeNames => NameLookupError::StoredAttributeName(source),
        DictionaryKind::EntityTypeNames => NameLookupError::StoredEntityTypeName(source),
    }
}

/// Whether the rows a
/// [`held_select_query`](crate::dictionary_statements::held_select_query)
/// returned may be published.
///
/// The flag is constant across the rows of one read; an empty read has nothing
/// to publish, so it withholds without consulting anything.
pub(crate) fn held_publication(rows: &[PgRow]) -> Result<CachePublication, NameLookupError> {
    match rows.first() {
        Some(row) if row.try_get::<bool, _>(SNAPSHOT_IS_COMMITTED_COLUMN)? => Ok(CachePublication::Publish),
        Some(_) | None => Ok(CachePublication::Withhold),
    }
}

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{dictionary_rows::stored_name_error, name_lookup::NameLookupError};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, reference::NgsiLdReferenceError};

    fn rejection() -> NgsiLdReferenceError {
        match "".parse::<AttributeName>() {
            Ok(name) => panic!("an empty name must not parse, got {name:?}"),
            Err(source) => source,
        }
    }

    #[test]
    fn an_unreadable_attribute_name_row_is_reported_against_the_attribute_dictionary() {
        assert!(matches!(
            stored_name_error::<AttributeName>(rejection()),
            NameLookupError::StoredAttributeName(_)
        ));
    }

    #[test]
    fn an_unreadable_entity_type_row_is_reported_against_the_entity_type_dictionary() {
        assert!(matches!(
            stored_name_error::<EntityTypeName>(rejection()),
            NameLookupError::StoredEntityTypeName(_)
        ));
    }
}

//! The rows of a statement reading several entities, cut into one run per
//! entity and decoded run by run.
//!
//! A statement reading a page of entities answers every entity's rows ordered
//! by the entity, each row naming it under [`READ_ENTITY_ID`]. A row that does
//! not read back makes its own entity unreadable and no other, so the runs are
//! decoded apart; the names every run's keys hold are still looked up once for
//! the whole read.

use crate::{
    error::TemporalStoreError,
    history_row_decode::{DecodedHistoryRow, decode_history_row},
    stored_catalog_error::StoredCatalogError,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_postgresql_attribute_instance::{name_keys::HeldKeys, name_resolution::names_of};
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use andromeda_postgresql_temporal_query::entity_reads::READ_ENTITY_ID;
use andromeda_temporal::port::history_reader::TemporalHistoryObservation;
use sqlx::{Row, postgres::PgRow};

/// The entity a row was read for.
///
/// # Errors
///
/// Returns [`TemporalStoreError`] when the row names no entity or one that
/// does not parse as an identifier.
pub(crate) fn entity_of(row: &PgRow) -> Result<Identifier, TemporalStoreError> {
    let raw: String = row.try_get(READ_ENTITY_ID)?;
    Ok(raw.parse::<Identifier>().map_err(StoredCatalogError::EntityId)?)
}

/// `rows`, ordered by entity, cut into the runs of each entity.
///
/// # Errors
///
/// Returns [`TemporalStoreError`] when a row does not name its entity.
pub(crate) fn by_entity(rows: &[PgRow]) -> Result<Vec<(Identifier, &[PgRow])>, TemporalStoreError> {
    let entities = rows.iter().map(entity_of).collect::<Result<Vec<_>, _>>()?;
    let mut runs: Vec<(Identifier, &[PgRow])> = Vec::new();
    let mut start = 0;
    for end in 1..=rows.len() {
        if entities.get(end) != entities.get(start)
            && let (Some(entity_id), Some(run)) = (entities.get(start), rows.get(start..end))
        {
            runs.push((entity_id.clone(), run)); // clone: each run owns the identifier it is keyed by
            start = end;
        }
    }
    Ok(runs)
}

/// The observations each of `runs` holds, or the failure that kept one of
/// its rows from reading back, the names of every key the runs hold looked
/// up once per dictionary.
///
/// # Errors
///
/// Returns [`TemporalStoreError`] when a dictionary cannot be read: no run's
/// members can then be named.
pub(crate) async fn assembled_entity_histories(
    runs: Vec<(Identifier, &[PgRow])>,
    attribute_names: &dyn NameLookup<AttributeName>,
    entity_type_names: &dyn NameLookup<EntityTypeName>,
) -> Result<Vec<(Identifier, Result<Vec<TemporalHistoryObservation>, TemporalStoreError>)>, TemporalStoreError> {
    let decoded: Vec<(Identifier, Result<Vec<DecodedHistoryRow>, TemporalStoreError>)> = runs
        .into_iter()
        .map(|(entity_id, rows)| (entity_id, rows.iter().map(decode_history_row).collect()))
        .collect();
    let mut held = HeldKeys::default();
    for row in decoded.iter().filter_map(|(_, rows)| rows.as_ref().ok()).flatten() {
        row.collect_keys(&mut held);
    }
    let names = names_of(&held.distinct(), attribute_names, entity_type_names).await?;
    Ok(decoded
        .into_iter()
        .map(|(entity_id, rows)| (entity_id, rows.and_then(|rows| rows.into_iter().map(|row| row.assemble(&names)).collect())))
        .collect())
}

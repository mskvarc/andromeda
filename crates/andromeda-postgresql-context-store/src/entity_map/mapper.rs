//! Row-to-domain conversions for Entity Map records.

use crate::{entity_map::lifecycle::MapControlRecord, error::ContextStoreError};
use andromeda_ngsi_ld_lexicon::identifier::{Identifier, IdentifierParseError};
use andromeda_ngsi_ld_model::{
    entity_map::{EntityMap, EntityMapSource, EntityMapSourceError},
    entity_map_fingerprint::{RequestFingerprint, RequestFingerprintError},
};
use indexmap::{IndexMap, IndexSet};
use thiserror::Error;

/// A stored Entity Map column that does not parse back into its domain type.
///
/// The broker only ever writes values it validated, so each variant is a column
/// holding something the broker never wrote.
#[derive(Debug, Error)]
pub enum StoredEntityMapError {
    /// The control record's request fingerprint is not a fingerprint.
    #[error("a stored entity map request fingerprint is not a fingerprint: {0}")]
    RequestFingerprint(#[source] RequestFingerprintError),
    /// An entry's entity identifier is not a URI.
    #[error("a stored entity map entry id is not a URI: {0}")]
    EntryId(#[source] IdentifierParseError),
    /// An entry's source is not an entity map source.
    #[error("a stored entity map entry source is not a source: {0}")]
    EntrySource(#[source] EntityMapSourceError),
    /// A link's registration identifier is not a URI.
    #[error("a stored entity map link registration is not a URI: {0}")]
    LinkRegistration(#[source] IdentifierParseError),
    /// A link's linked map identifier is not a URI.
    #[error("a stored linked entity map id is not a URI: {0}")]
    LinkedMap(#[source] IdentifierParseError),
}

/// Assemble a map from its control record and the child rows read for it.
pub(crate) fn rows_to_entity_map(
    id: Identifier,
    control: &MapControlRecord,
    entries: Vec<(String, String)>,
    links: Vec<(String, String)>,
) -> Result<EntityMap, ContextStoreError> {
    let request_fingerprint = control
        .request_fingerprint
        .as_deref()
        .map(|stored| stored.parse::<RequestFingerprint>().map_err(StoredEntityMapError::RequestFingerprint))
        .transpose()?;
    let mut entity_map: IndexMap<Identifier, IndexSet<EntityMapSource>> = IndexMap::new();
    for (entity_id, source) in entries {
        let entity_id = entity_id.parse::<Identifier>().map_err(StoredEntityMapError::EntryId)?;
        let source = EntityMapSource::try_from(source).map_err(StoredEntityMapError::EntrySource)?;
        entity_map.entry(entity_id).or_default().insert(source);
    }
    let mut linked_maps = IndexMap::new();
    for (registration_id, linked_map_id) in links {
        let registration_id = registration_id.parse::<Identifier>().map_err(StoredEntityMapError::LinkRegistration)?;
        let linked_map_id = linked_map_id.parse::<Identifier>().map_err(StoredEntityMapError::LinkedMap)?;
        linked_maps.insert(registration_id, linked_map_id);
    }
    Ok(EntityMap::builder()
        .id(id)
        .expires_at(control.expires_at)
        .entity_map(entity_map)
        .linked_maps(linked_maps)
        .request_fingerprint(request_fingerprint)
        .build())
}

#[cfg(test)]
mod tests {
    use crate::entity_map::{lifecycle::MapControlRecord, mapper::rows_to_entity_map};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{
        entity_map::EntityMapSource,
        entity_map_fingerprint::{MapRequestKind, RequestFingerprint},
    };
    use chrono::Utc;

    fn control(request_fingerprint: Option<String>) -> MapControlRecord {
        MapControlRecord {
            expires_at: Utc::now(),
            request_fingerprint,
        }
    }

    #[test]
    fn entries_regroup_by_entity_in_row_order_and_links_parse() {
        let fingerprint = RequestFingerprint::new(MapRequestKind::QueryEntities, [9; 32]);
        let map = rows_to_entity_map(
            "urn:ngsi-ld:EntityMap:1".parse().expect("valid URI"),
            &control(Some(fingerprint.to_string())),
            vec![
                ("urn:ngsi-ld:Vehicle:2".to_string(), "@none".to_string()),
                ("urn:ngsi-ld:Vehicle:1".to_string(), "@none".to_string()),
                ("urn:ngsi-ld:Vehicle:2".to_string(), "urn:ngsi-ld:csr:b".to_string()),
            ],
            vec![("urn:ngsi-ld:csr:b".to_string(), "urn:ngsi-ld:EntityMap:b".to_string())],
        )
        .expect("maps");
        let ids: Vec<&str> = map.entity_ids().map(Identifier::as_str).collect();
        pretty_assertions::assert_eq!(ids, vec!["urn:ngsi-ld:Vehicle:2", "urn:ngsi-ld:Vehicle:1"]);
        pretty_assertions::assert_eq!(
            map.entity_map
                .get(&"urn:ngsi-ld:Vehicle:2".parse::<Identifier>().expect("valid URI"))
                .map(|sources| sources.iter().collect::<Vec<_>>()),
            Some(vec![
                &EntityMapSource::Local,
                &EntityMapSource::Registration("urn:ngsi-ld:csr:b".parse().expect("valid URI"))
            ])
        );
        pretty_assertions::assert_eq!(map.linked_maps.len(), 1);
        pretty_assertions::assert_eq!(map.request_fingerprint, Some(fingerprint));
    }

    #[test]
    fn a_map_recorded_without_a_fingerprint_reads_back_without_one_and_a_corrupt_one_is_internal() {
        let map = rows_to_entity_map("urn:ngsi-ld:EntityMap:1".parse().expect("valid URI"), &control(None), Vec::new(), Vec::new()).expect("maps");
        assert!(map.request_fingerprint.is_none());
        assert!(
            rows_to_entity_map(
                "urn:ngsi-ld:EntityMap:1".parse().expect("valid URI"),
                &control(Some("garbage".to_string())),
                Vec::new(),
                Vec::new()
            )
            .is_err()
        );
    }

    #[test]
    fn an_invalid_source_token_is_an_internal_error() {
        assert!(
            rows_to_entity_map(
                "urn:ngsi-ld:EntityMap:1".parse().expect("valid URI"),
                &control(None),
                vec![("urn:ngsi-ld:Vehicle:1".to_string(), "not a uri".to_string())],
                Vec::new(),
            )
            .is_err()
        );
    }
}

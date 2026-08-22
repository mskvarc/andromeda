//! Row-to-domain conversions for Entity Map records.

use crate::{entity_map::lifecycle::MapControlRecord, error::ContextStoreError};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    entity_map::{EntityMap, EntityMapSource},
    entity_map_fingerprint::RequestFingerprint,
};
use indexmap::IndexMap;

/// Assemble a map from its control record and the child rows read for it.
pub(crate) fn rows_to_entity_map(
    id: Uri,
    control: MapControlRecord,
    entries: Vec<(String, String)>,
    links: Vec<(String, String)>,
) -> Result<EntityMap, ContextStoreError> {
    let request_fingerprint = control
        .request_fingerprint
        .as_deref()
        .map(|stored| {
            stored
                .parse::<RequestFingerprint>()
                .map_err(|error| ContextStoreError::Internal(format!("Invalid entity map request fingerprint: {error}")))
        })
        .transpose()?;
    let mut entity_map: IndexMap<Uri, Vec<EntityMapSource>> = IndexMap::new();
    for (entity_id, source) in entries {
        let entity_id = entity_id
            .parse::<Uri>()
            .map_err(|error| ContextStoreError::Internal(format!("Invalid entity map entry id '{entity_id}': {error}")))?;
        let source = EntityMapSource::try_from(source).map_err(|error| ContextStoreError::Internal(format!("Invalid entity map source: {error}")))?;
        entity_map.entry(entity_id).or_default().push(source);
    }
    let mut linked_maps = IndexMap::new();
    for (registration_id, linked_map_id) in links {
        let registration_id = registration_id
            .parse::<Uri>()
            .map_err(|error| ContextStoreError::Internal(format!("Invalid entity map link registration '{registration_id}': {error}")))?;
        let linked_map_id = linked_map_id
            .parse::<Uri>()
            .map_err(|error| ContextStoreError::Internal(format!("Invalid linked entity map id '{linked_map_id}': {error}")))?;
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
    use andromeda_ngsi_ld_lexicon::uri::Uri;
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
            control(Some(fingerprint.to_string())),
            vec![
                ("urn:ngsi-ld:Vehicle:2".to_string(), "@none".to_string()),
                ("urn:ngsi-ld:Vehicle:1".to_string(), "@none".to_string()),
                ("urn:ngsi-ld:Vehicle:2".to_string(), "urn:ngsi-ld:csr:b".to_string()),
            ],
            vec![("urn:ngsi-ld:csr:b".to_string(), "urn:ngsi-ld:EntityMap:b".to_string())],
        )
        .expect("maps");
        let ids: Vec<&str> = map.entity_ids().map(|id| id.as_str()).collect();
        pretty_assertions::assert_eq!(ids, vec!["urn:ngsi-ld:Vehicle:2", "urn:ngsi-ld:Vehicle:1"]);
        pretty_assertions::assert_eq!(
            map.entity_map.get(&"urn:ngsi-ld:Vehicle:2".parse::<Uri>().expect("valid URI")),
            Some(&vec![
                EntityMapSource::Local,
                EntityMapSource::Registration("urn:ngsi-ld:csr:b".parse().expect("valid URI"))
            ])
        );
        pretty_assertions::assert_eq!(map.linked_maps.len(), 1);
        pretty_assertions::assert_eq!(map.request_fingerprint, Some(fingerprint));
    }

    #[test]
    fn a_map_recorded_without_a_fingerprint_reads_back_without_one_and_a_corrupt_one_is_internal() {
        let map = rows_to_entity_map("urn:ngsi-ld:EntityMap:1".parse().expect("valid URI"), control(None), Vec::new(), Vec::new()).expect("maps");
        assert!(map.request_fingerprint.is_none());
        assert!(
            rows_to_entity_map(
                "urn:ngsi-ld:EntityMap:1".parse().expect("valid URI"),
                control(Some("garbage".to_string())),
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
                control(None),
                vec![("urn:ngsi-ld:Vehicle:1".to_string(), "not a uri".to_string())],
                Vec::new(),
            )
            .is_err()
        );
    }
}

//! The number the `entity_maps.request_kind` column stores the operation that
//! created a map as, and the operation a stored number names.
//!
//! Both directions are exhaustive matches, so an operation added to the domain
//! fails to compile here, where its persisted number is decided.

use andromeda_ngsi_ld_model::entity_map_fingerprint::MapRequestKind;
use andromeda_postgresql_stored_values::entity_map_request_kind::EntityMapRequestKindCode;

/// The registry member the store persists for the operation that created a map.
#[must_use]
pub const fn stored_kind(kind: MapRequestKind) -> EntityMapRequestKindCode {
    match kind {
        MapRequestKind::QueryEntities => EntityMapRequestKindCode::QueryEntities,
        MapRequestKind::RetrieveEntity => EntityMapRequestKindCode::RetrieveEntity,
        MapRequestKind::QueryTemporal => EntityMapRequestKindCode::QueryTemporal,
        MapRequestKind::RetrieveTemporal => EntityMapRequestKindCode::RetrieveTemporal,
    }
}

/// The operation a stored registry member names.
#[must_use]
pub const fn request_kind(code: EntityMapRequestKindCode) -> MapRequestKind {
    match code {
        EntityMapRequestKindCode::QueryEntities => MapRequestKind::QueryEntities,
        EntityMapRequestKindCode::RetrieveEntity => MapRequestKind::RetrieveEntity,
        EntityMapRequestKindCode::QueryTemporal => MapRequestKind::QueryTemporal,
        EntityMapRequestKindCode::RetrieveTemporal => MapRequestKind::RetrieveTemporal,
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_map::request_kind_code::{request_kind, stored_kind};
    use andromeda_ngsi_ld_model::entity_map_fingerprint::MapRequestKind;
    use andromeda_postgresql_stored_values::entity_map_request_kind::EntityMapRequestKindCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn every_operation_is_stored_under_its_documented_number_and_read_back() {
        let operations = [
            (MapRequestKind::QueryEntities, 1),
            (MapRequestKind::RetrieveEntity, 2),
            (MapRequestKind::QueryTemporal, 3),
            (MapRequestKind::RetrieveTemporal, 4),
        ];
        assert_eq!(operations.len(), EntityMapRequestKindCode::ALL.len());
        for (operation, number) in operations {
            assert_eq!(stored_kind(operation).code(), number);
            assert_eq!(request_kind(stored_kind(operation)), operation);
        }
    }
}

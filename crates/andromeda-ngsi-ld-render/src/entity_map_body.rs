//! The clause 5.2.39 `EntityMap` as a response body, written for the NGSI-LD
//! version a client asked for.
//!
//! Tables 6.32.3.1-1 and 6.34.3.1-1 answer an `EntityMap` with 203 when it was
//! amended to an older version (clause 4.3.6.8). Of the fallback tables, only
//! the `expiresAt` row of Table 4.3.6.8-1 names a member an `EntityMap`
//! carries; it arrived at 1.9, so an older target receives the map without it.

use crate::version::{
    adapted_payload::AdaptedPayload,
    fallback::{EntityMember, VersionFallback},
};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, spec_version::SpecVersion, time::instant_serde};
use andromeda_ngsi_ld_model::entity_map::{EntityMap, EntityMapSource, EntityMapType};
use chrono::{DateTime, Utc};
use indexmap::{IndexMap, IndexSet};
use serde::Serialize;

/// An `EntityMap`, written for the version a `Prefer: ngsi-ld=<version>`
/// requested.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityMapBody<'a> {
    /// The map's identifier.
    id: &'a Identifier,
    /// Always `EntityMap`.
    #[serde(rename = "type")]
    type_: &'a EntityMapType,
    /// When the map stops being usable, unless the target predates the
    /// member.
    #[serde(with = "instant_serde::optional", skip_serializing_if = "Option::is_none")]
    expires_at: Option<DateTime<Utc>>,
    /// Every entity of the map with the sources that hold it.
    entity_map: &'a IndexMap<Identifier, IndexSet<EntityMapSource>>,
    /// The `EntityMap` each contributing context source created.
    linked_maps: &'a IndexMap<Identifier, Identifier>,
}

impl<'a> EntityMapBody<'a> {
    /// `map`, written for `requested`, or for this broker's own version when
    /// no version was requested.
    #[must_use]
    pub fn new(map: &'a EntityMap, requested: Option<SpecVersion>) -> EntityMapBody<'a> {
        let keeps_expiry = requested
            .and_then(|target| VersionFallback::new(target, AdaptedPayload::Entity))
            .is_none_or(|fallback| fallback.keeps_entity_member(EntityMember::ExpiresAt));
        EntityMapBody {
            id: &map.id,
            type_: &map.type_,
            expires_at: keeps_expiry.then_some(map.expires_at),
            entity_map: &map.entity_map,
            linked_maps: &map.linked_maps,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_map_body::EntityMapBody;
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
    use andromeda_ngsi_ld_model::entity_map::EntityMap;
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn map() -> EntityMap {
        EntityMap::local(
            "urn:ngsi-ld:EntityMap:1".parse().expect("an identifier"),
            Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).single().expect("an instant"),
            ["urn:ngsi-ld:Building:1".parse().expect("an identifier")],
        )
    }

    fn written(requested: Option<SpecVersion>) -> Value {
        let map = map();
        serde_json::from_str(&serde_json::to_string(&EntityMapBody::new(&map, requested)).expect("the map is written")).expect("the body is JSON")
    }

    /// Without a requested version, or for the conformant one, the body is the
    /// map's own clause 5.2.39 representation.
    #[test]
    fn the_conformant_body_is_the_map_itself() {
        let own: Value = serde_json::from_str(&serde_json::to_string(&map()).expect("the map serializes")).expect("JSON");
        assert_eq!(written(None), own);
        assert_eq!(written(Some(SpecVersion::CONFORMANT)), own);
    }

    /// Table 4.3.6.8-1: `expiresAt` arrived at 1.9.
    #[test]
    fn an_older_target_receives_the_map_without_its_expiry() {
        assert_eq!(
            written(Some(SpecVersion::V1_8)),
            json!({
                "id": "urn:ngsi-ld:EntityMap:1",
                "type": "EntityMap",
                "entityMap": {"urn:ngsi-ld:Building:1": ["@none"]},
                "linkedMaps": {}
            })
        );
    }
}

//! What a read does about Entity Maps (clause 5.5.14, clauses 5.7.1.4 to
//! 5.7.4.4, Table 6.4.3.2-3).
//!
//! A request may reference a map and may ask for one. A live referenced map
//! is used and echoed, never minted again; a referenced map that expired or
//! never existed "yields no inference" and a new one is created whether or
//! not the request asked; a request that asks for a map and references none
//! gets one minted. Table 6.4.3.2-3 answers 201 exactly when a map was
//! "(re)created".

use crate::{error::EntityMapError, service::ReferencedEntityMap};
use andromeda_ngsi_ld_model::entity_map::EntityMap;

/// Whether the request asked for the map's location (Table 6.4.3.2-1 `entityMap`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityMapFlag {
    /// `entityMap=true`.
    Requested,
    /// `entityMap` absent or `false`.
    NotRequested,
}

impl EntityMapFlag {
    /// Read the optional boolean request parameter.
    pub const fn from_parameter(value: Option<bool>) -> EntityMapFlag {
        match value {
            Some(true) => EntityMapFlag::Requested,
            Some(false) | None => EntityMapFlag::NotRequested,
        }
    }
}

/// The map a read operates under.
#[derive(Debug)]
pub enum EntityMapUse {
    /// A live referenced map: it fixes the entities and sources considered
    /// and is echoed back; the response is a 200.
    Reuse(Box<EntityMap>),
    /// No live map is in play and one is wanted: the map is minted before
    /// the read, so every forwarded request can carry its location, filled
    /// after it and stored; the response is a 201 where the binding defines
    /// one.
    Mint(Box<EntityMap>),
    /// No map is referenced and none is wanted.
    None,
}

impl EntityMapUse {
    /// Decide from the referenced map, if the request named one, and the flag,
    /// minting the fresh map with `mint` when one is wanted.
    pub fn decide(
        referenced: Option<ReferencedEntityMap>,
        flag: EntityMapFlag,
        mint: impl FnOnce() -> Result<EntityMap, EntityMapError>,
    ) -> Result<EntityMapUse, EntityMapError> {
        Ok(match (referenced, flag) {
            (Some(ReferencedEntityMap::Live(map)), EntityMapFlag::Requested | EntityMapFlag::NotRequested) => EntityMapUse::Reuse(map),
            (Some(ReferencedEntityMap::Unavailable), EntityMapFlag::Requested | EntityMapFlag::NotRequested) | (None, EntityMapFlag::Requested) => {
                EntityMapUse::Mint(Box::new(mint()?))
            }
            (None, EntityMapFlag::NotRequested) => EntityMapUse::None,
        })
    }

    /// Whether the read ends by creating a map.
    pub const fn mints(&self) -> bool {
        matches!(self, EntityMapUse::Mint(_))
    }

    /// The live map the read runs under, if it reuses one.
    pub fn reused(&self) -> Option<&EntityMap> {
        match self {
            EntityMapUse::Reuse(map) => Some(map),
            EntityMapUse::Mint(_) | EntityMapUse::None => None,
        }
    }

    /// The map the read is minting, if it mints one.
    pub fn minting(&self) -> Option<&EntityMap> {
        match self {
            EntityMapUse::Mint(map) => Some(map),
            EntityMapUse::Reuse(_) | EntityMapUse::None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::EntityMapError,
        reuse::{EntityMapFlag, EntityMapUse},
        service::ReferencedEntityMap,
    };
    use andromeda_ngsi_ld_model::entity_map::EntityMap;
    use chrono::{TimeZone, Utc};

    fn map(id: &str) -> EntityMap {
        EntityMap::local(
            id.parse().expect("valid URI"),
            Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).single().expect("valid instant"),
            [],
        )
    }

    fn live() -> ReferencedEntityMap {
        ReferencedEntityMap::Live(Box::new(map("urn:ngsi-ld:EntityMap:live")))
    }

    fn fresh() -> Result<EntityMap, EntityMapError> {
        Ok(map("urn:ngsi-ld:EntityMap:fresh"))
    }

    #[test]
    fn the_flag_reads_the_optional_parameter() {
        pretty_assertions::assert_eq!(EntityMapFlag::from_parameter(Some(true)), EntityMapFlag::Requested);
        pretty_assertions::assert_eq!(EntityMapFlag::from_parameter(Some(false)), EntityMapFlag::NotRequested);
        pretty_assertions::assert_eq!(EntityMapFlag::from_parameter(None), EntityMapFlag::NotRequested);
    }

    /// The decision table of clause 5.5.14 and Table 6.4.3.2-3: a live map
    /// is reused whatever the flag, an unavailable one is replaced whatever
    /// the flag, and without a reference the flag alone decides.
    #[test]
    fn a_live_map_is_reused_and_never_re_minted() -> Result<(), EntityMapError> {
        for flag in [EntityMapFlag::Requested, EntityMapFlag::NotRequested] {
            let decided = EntityMapUse::decide(Some(live()), flag, fresh)?;
            assert!(matches!(&decided, EntityMapUse::Reuse(map) if map.id.as_str() == "urn:ngsi-ld:EntityMap:live"));
            assert!(!decided.mints());
            assert!(decided.reused().is_some());
            assert!(decided.minting().is_none());
        }
        Ok(())
    }

    #[test]
    fn an_unavailable_map_is_recreated_whatever_the_flag() -> Result<(), EntityMapError> {
        for flag in [EntityMapFlag::Requested, EntityMapFlag::NotRequested] {
            let decided = EntityMapUse::decide(Some(ReferencedEntityMap::Unavailable), flag, fresh)?;
            assert!(decided.mints());
            assert!(decided.reused().is_none());
            pretty_assertions::assert_eq!(decided.minting().map(|map| map.id.as_str()), Some("urn:ngsi-ld:EntityMap:fresh"));
        }
        Ok(())
    }

    #[test]
    fn without_a_reference_the_flag_decides() -> Result<(), EntityMapError> {
        assert!(EntityMapUse::decide(None, EntityMapFlag::Requested, fresh)?.mints());
        assert!(matches!(EntityMapUse::decide(None, EntityMapFlag::NotRequested, fresh)?, EntityMapUse::None));
        Ok(())
    }
}

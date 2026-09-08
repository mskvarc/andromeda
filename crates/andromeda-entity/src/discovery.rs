//! Entity type and attribute discovery (ETSI GS CIM 009 clauses 5.7.5 to
//! 5.7.10).
//!
//! Each retrieval reads the local store and, unless the request is confined to
//! this broker, unions it with what the Context Registry says is possibly
//! available (clause 5.7.11). The retrievals live one per module; this module
//! owns the service and the two choices every retrieval takes.

use crate::port::{discovery::EntityDiscoveryStore, registry_types::RegistryTypeSource};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use std::sync::Arc;

mod attribute_info;
mod attribute_list;
mod compaction_fixups;
mod registry_union;
mod stored_names;
mod type_details;
mod type_info;
mod type_list;

/// Service for NGSI-LD entity type and attribute discovery operations.
pub struct EntityDiscoveryService {
    store: Arc<dyn EntityDiscoveryStore>,
    registry: Arc<dyn RegistryTypeSource>,
    engine: JsonLdEngine,
}

/// Representation requested for an attribute collection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttributeCollectionRepresentation {
    /// Return identifiers only.
    List,
    /// Return the detailed resource representation.
    Detailed,
}

/// How far a discovery looks (clauses 5.7.11 and 5.5.13).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscoveryScope {
    /// Only entities this broker stores: the `local=true` request.
    Local,
    /// The local store unioned with the Context Registry's possibly
    /// available types and attributes.
    IncludingRegistry,
}

impl DiscoveryScope {
    /// The scope a request's `local` parameter asks for (clause 5.5.13): only
    /// an explicit `local=true` confines the discovery to this broker.
    #[must_use]
    pub const fn for_local_request(local: Option<bool>) -> DiscoveryScope {
        match local {
            Some(true) => DiscoveryScope::Local,
            Some(false) | None => DiscoveryScope::IncludingRegistry,
        }
    }
}

impl EntityDiscoveryService {
    /// Create a discovery service over the local store and the Context
    /// Registry's view of what is possibly available.
    pub fn new(store: Arc<dyn EntityDiscoveryStore>, registry: Arc<dyn RegistryTypeSource>, engine: JsonLdEngine) -> EntityDiscoveryService {
        EntityDiscoveryService { store, registry, engine }
    }
}

#[cfg(test)]
mod tests {
    use crate::discovery::DiscoveryScope;

    /// Clause 5.5.13: only `local=true` confines the discovery.
    #[test]
    fn only_an_explicit_local_true_confines_the_discovery() {
        pretty_assertions::assert_eq!(DiscoveryScope::for_local_request(Some(true)), DiscoveryScope::Local);
        pretty_assertions::assert_eq!(DiscoveryScope::for_local_request(Some(false)), DiscoveryScope::IncludingRegistry);
        pretty_assertions::assert_eq!(DiscoveryScope::for_local_request(None), DiscoveryScope::IncludingRegistry);
    }
}

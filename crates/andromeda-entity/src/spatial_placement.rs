//! Where one entity's location belongs in the entity spatial index.
//!
//! The index holds the default `location` `GeoProperty` only, and holds it as
//! the store evaluates it: the one geometry the store hoists from the payload
//! into its search projection. Every path that feeds the index -- the commit
//! path, a rebuild and a catch-up -- places an entity through this one module,
//! so the three can never disagree about which partitions an entity occupies.
//! A geo query naming any other `GeoProperty` is never narrowed and is
//! evaluated by the store in full.

use crate::{entity_invariant::EntityInvariant, error::EntityError};
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use andromeda_spatial_index::key::SpatialKey;
use andromeda_tenancy::tenant_ref::TenantRef;
use iri_rs::IriBuf;
use tracing::debug;

/// The partitions of one tenant's index an entity's location occupies.
#[derive(Debug, Clone)]
pub struct LocationPlacement {
    /// The tenant whose index this is.
    tenant: TenantRef,
    /// The default `location` `GeoProperty`, the one the index holds.
    location: AttributeName,
}

impl LocationPlacement {
    /// The placement of locations in `tenant`'s index.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Internal`] when the core `location` term is not a
    /// well-formed attribute name.
    pub fn new(tenant: TenantRef) -> Result<LocationPlacement, EntityError> {
        let location = expanded_properties::LOCATION
            .as_str()
            .parse()
            .map_err(|error| EntityError::from(EntityInvariant::DefaultLocationName(error)))?;
        Ok(LocationPlacement { tenant, location })
    }

    /// The tenant whose index this is.
    #[must_use]
    pub const fn tenant(&self) -> &TenantRef {
        &self.tenant
    }

    /// The `GeoProperty` the index holds.
    #[must_use]
    pub const fn geoproperty(&self) -> &AttributeName {
        &self.location
    }

    /// One `(partition, geometry)` pair per entity type, or none when the
    /// entity conveys no location. Every pair borrows the one geometry: the
    /// index keeps only its projection.
    ///
    /// A type whose name is not an absolute IRI cannot name a partition a
    /// query would read, since queries select expanded types, and is skipped.
    #[must_use]
    pub fn of<'g>(&self, entity_types: &[EntityTypeName], location: Option<&'g GeometryForm>) -> Vec<(SpatialKey, &'g GeometryForm)> {
        let Some(location) = location else {
            return Vec::new();
        };
        entity_types
            .iter()
            .filter_map(|entity_type| match IriBuf::new(entity_type.as_str().to_owned()) {
                Ok(type_iri) => Some(type_iri),
                Err(error) => {
                    debug!(entity_type = %entity_type, %error, "spatial placement skipped an unexpanded entity type");
                    None
                }
            })
            .map(|type_iri| {
                (
                    SpatialKey::new(self.tenant.clone(), type_iri, self.location.clone()), // clone: each partition key owns its tenant and geoproperty
                    location,
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::spatial_placement::LocationPlacement;
    use andromeda_ngsi_ld_geometry::{form::GeometryForm, position::Position};
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_tenancy::tenant_ref::TenantRef;
    use pretty_assertions::assert_eq;

    fn types(raw: &[&str]) -> Vec<EntityTypeName> {
        raw.iter().map(|name| name.parse().expect("a well-formed type name")).collect()
    }

    #[test]
    fn a_located_entity_occupies_one_location_partition_per_type() {
        let placement = LocationPlacement::new(TenantRef::UnnamedDefault).expect("the core location term is an attribute name");
        let point = GeometryForm::Point(Position::try_new(1.0, 2.0, None).expect("a test position lies on the CRS84 axes"));
        let placed = placement.of(&types(&["https://example.org/Car", "https://example.org/Asset"]), Some(&point));
        let keys: Vec<(String, String)> = placed
            .iter()
            .map(|(key, _)| (key.type_iri.as_str().to_owned(), key.geoproperty.as_str().to_owned()))
            .collect();
        assert_eq!(
            keys,
            vec![
                ("https://example.org/Car".to_owned(), "https://uri.etsi.org/ngsi-ld/location".to_owned()),
                ("https://example.org/Asset".to_owned(), "https://uri.etsi.org/ngsi-ld/location".to_owned()),
            ]
        );
    }

    #[test]
    fn an_entity_without_a_location_occupies_no_partition() {
        let placement = LocationPlacement::new(TenantRef::UnnamedDefault).expect("the core location term is an attribute name");
        assert_eq!(placement.of(&types(&["https://example.org/Car"]), None).len(), 0);
    }
}

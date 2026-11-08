//! The expanded names NGSI-LD reserves, as the terms compaction asks about.
//!
//! The walk selects a key for each reserved member of an instance, its value
//! carrier, `observedAt`, `datasetId` and the rest (clause 4.5), and for each
//! family it writes as an `@type`. The terms are built once for the process.

use andromeda_ngsi_ld_entity::document::value_carrier::ValueCarrier;
use andromeda_ngsi_ld_geometry::geometry_type::GeometryType;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_vocabulary::expanded::{classes, properties};
use iri_rs::Iri;
use jsonld::{BlankIdBuf, Id, IriBuf, Term};
use std::sync::LazyLock;

/// A term for an expanded IRI.
pub(crate) type IriTerm = Term<IriBuf, BlankIdBuf>;

/// The reserved terms, built on first use.
static RESERVED: LazyLock<ReservedTerms> = LazyLock::new(ReservedTerms::new);

/// The reserved terms.
pub(crate) fn reserved() -> &'static ReservedTerms {
    &RESERVED
}

/// The expanded names the walk writes reserved members under.
#[derive(Debug)]
pub(crate) struct ReservedTerms {
    /// `ngsi-ld:DateTime`, the datatype of every clause 4.8 and 4.22 time.
    pub(crate) date_time: IriBuf,
    /// `scope` (clause 4.18).
    pub(crate) scope: IriTerm,
    /// `expiresAt` (clause 4.22).
    pub(crate) expires_at: IriTerm,
    /// `observedAt`.
    pub(crate) observed_at: IriTerm,
    /// `createdAt` (clause 4.8).
    pub(crate) created_at: IriTerm,
    /// `modifiedAt` (clause 4.8).
    pub(crate) modified_at: IriTerm,
    /// `deletedAt` (clause 4.8).
    pub(crate) deleted_at: IriTerm,
    /// `datasetId` (clause 4.5.5).
    pub(crate) dataset_id: IriTerm,
    /// `instanceId` (clause 4.5.7).
    pub(crate) instance_id: IriTerm,
    /// `unitCode`.
    pub(crate) unit_code: IriTerm,
    /// `valueType`.
    pub(crate) value_type: IriTerm,
    /// `objectType`.
    pub(crate) object_type: IriTerm,
    /// `entityIdSealed` (clause 4.5.2.2).
    pub(crate) entity_id_sealed: IriTerm,
    /// `entityTypeSealed` (clause 4.5.2.2).
    pub(crate) entity_type_sealed: IriTerm,
    /// The `GeoJSON` `coordinates` member.
    pub(crate) coordinates: IriTerm,
    /// The `GeoJSON` `bbox` member (IETF RFC 7946 section 5).
    pub(crate) bbox: IriTerm,
    /// The value carriers, in [`ValueCarrier`] order.
    carriers: [IriTerm; 7],
    /// The family types, in [`AttributeType`] order.
    families: [IriTerm; 8],
    /// The `GeoJSON` geometry types, in [`GeometryType`] order.
    geometry_types: [IriTerm; 6],
}

impl ReservedTerms {
    /// Build every reserved term.
    fn new() -> ReservedTerms {
        ReservedTerms {
            date_time: IriBuf::from(classes::DATE_TIME),
            scope: term(properties::SCOPE),
            expires_at: term(properties::EXPIRES_AT),
            observed_at: term(properties::OBSERVED_AT),
            created_at: term(properties::CREATED_AT),
            modified_at: term(properties::MODIFIED_AT),
            deleted_at: term(properties::DELETED_AT),
            dataset_id: term(properties::DATASET_ID),
            instance_id: term(properties::INSTANCE_ID),
            unit_code: term(properties::UNIT_CODE),
            value_type: term(properties::VALUE_TYPE),
            object_type: term(properties::OBJECT_TYPE),
            entity_id_sealed: term(properties::ENTITY_ID_SEALED),
            entity_type_sealed: term(properties::ENTITY_TYPE_SEALED),
            coordinates: term(properties::COORDINATES),
            bbox: term(properties::BBOX),
            carriers: CARRIERS.map(|carrier| term(carrier.iri())),
            families: FAMILIES.map(|family| term(family.iri())),
            geometry_types: [
                classes::POINT,
                classes::MULTI_POINT,
                classes::LINE_STRING,
                classes::MULTI_LINE_STRING,
                classes::POLYGON,
                classes::MULTI_POLYGON,
            ]
            .map(term),
        }
    }

    /// The `@type` term of a geometry node of `geometry_type`.
    pub(crate) const fn geometry_type(&self, geometry_type: GeometryType) -> &IriTerm {
        let [point, multi_point, line_string, multi_line_string, polygon, multi_polygon] = &self.geometry_types;
        match geometry_type {
            GeometryType::Point => point,
            GeometryType::MultiPoint => multi_point,
            GeometryType::LineString => line_string,
            GeometryType::MultiLineString => multi_line_string,
            GeometryType::Polygon => polygon,
            GeometryType::MultiPolygon => multi_polygon,
        }
    }

    /// The term `carrier` is written under.
    pub(crate) const fn carrier(&self, carrier: ValueCarrier) -> &IriTerm {
        let [value, object, language_map, vocab, json, value_list, object_list] = &self.carriers;
        match carrier {
            ValueCarrier::Value => value,
            ValueCarrier::Object => object,
            ValueCarrier::LanguageMap => language_map,
            ValueCarrier::Vocab => vocab,
            ValueCarrier::Json => json,
            ValueCarrier::ValueList => value_list,
            ValueCarrier::ObjectList => object_list,
        }
    }

    /// The `@type` term of `family`.
    pub(crate) const fn family(&self, family: AttributeType) -> &IriTerm {
        let [
            property,
            relationship,
            geo_property,
            language_property,
            vocab_property,
            json_property,
            list_property,
            list_relationship,
        ] = &self.families;
        match family {
            AttributeType::Property => property,
            AttributeType::Relationship => relationship,
            AttributeType::GeoProperty => geo_property,
            AttributeType::LanguageProperty => language_property,
            AttributeType::VocabProperty => vocab_property,
            AttributeType::JsonProperty => json_property,
            AttributeType::ListProperty => list_property,
            AttributeType::ListRelationship => list_relationship,
        }
    }
}

/// Every value carrier.
const CARRIERS: [ValueCarrier; 7] = [
    ValueCarrier::Value,
    ValueCarrier::Object,
    ValueCarrier::LanguageMap,
    ValueCarrier::Vocab,
    ValueCarrier::Json,
    ValueCarrier::ValueList,
    ValueCarrier::ObjectList,
];

/// Every attribute family.
const FAMILIES: [AttributeType; 8] = [
    AttributeType::Property,
    AttributeType::Relationship,
    AttributeType::GeoProperty,
    AttributeType::LanguageProperty,
    AttributeType::VocabProperty,
    AttributeType::JsonProperty,
    AttributeType::ListProperty,
    AttributeType::ListRelationship,
];

/// The term for a vocabulary IRI.
fn term(iri: Iri<&'static str>) -> IriTerm {
    Term::Id(Id::iri(IriBuf::from(iri)))
}

#[cfg(test)]
mod tests {
    use crate::reserved_terms::{CARRIERS, FAMILIES, reserved};

    #[test]
    fn every_carrier_and_family_finds_its_own_term() {
        for carrier in CARRIERS {
            pretty_assertions::assert_eq!(reserved().carrier(carrier).as_str(), carrier.iri().as_str());
        }
        for family in FAMILIES {
            pretty_assertions::assert_eq!(reserved().family(family).as_str(), family.iri().as_str());
        }
    }
}

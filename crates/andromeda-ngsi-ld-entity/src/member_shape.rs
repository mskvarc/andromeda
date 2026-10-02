//! Where in an expanded document a member was not shaped as its expanded
//! form shapes it, as a refusal names the place.

use strum::Display;

/// Where in an expanded document a member was not shaped the way the expanded
/// form of clause 5.5.7 shapes it.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, PartialEq)]
#[strum(serialize_all = "camelCase")]
pub enum MemberShape {
    /// The document root: one entity node, bare or in a one-element array.
    Root,
    /// The entity `@id`.
    EntityId,
    /// The entity `@type`.
    EntityType,
    /// The `scope` member.
    Scope,
    /// The entity-level `expiresAt` member.
    EntityExpiresAt,
    /// An attribute member: an array of instance objects.
    Attribute,
    /// An attribute instance's `@type`.
    InstanceType,
    /// `datasetId`.
    DatasetId,
    /// `instanceId`.
    InstanceId,
    /// `observedAt`.
    ObservedAt,
    /// `expiresAt` of an instance.
    ExpiresAt,
    /// `createdAt`.
    CreatedAt,
    /// `modifiedAt`.
    ModifiedAt,
    /// `deletedAt`.
    DeletedAt,
    /// `unitCode`.
    UnitCode,
    /// `valueType`.
    ValueType,
    /// `objectType`.
    ObjectType,
    /// `value`.
    Value,
    /// `object`.
    Object,
    /// `languageMap`.
    LanguageMap,
    /// `vocab`.
    Vocab,
    /// `json`.
    Json,
    /// `valueList`.
    ValueList,
    /// `objectList`.
    ObjectList,
    /// A data-integrity seal of clause 4.5.2.2.
    ProofSeal,
    /// A value object, list object or node inside a value.
    NestedValue,
    /// The `GeoJSON` node of a geometry.
    Geometry,
}

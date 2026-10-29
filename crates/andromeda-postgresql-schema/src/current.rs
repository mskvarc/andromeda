//! The relations that hold current entity state.
//!
//! The `entities` row and the `current_attributes` rows of its attributes are
//! the current-state grain a context statement and a temporal statement both
//! read, together with the `e` alias they read it under.

use sea_query::Iden;

/// The `entities` table - the identity and lifecycle of one current Entity.
///
/// The three relations `entities`, `current_attributes` and `entity_search`
/// together hold one Entity's current state, split by write pattern: identity,
/// expiry and the write count here, one row per attribute instance in
/// `current_attributes`, the indexed query axes in `entity_search`. All three
/// share `entity_key`.
#[derive(Iden, Clone, Copy)]
pub enum Entities {
    /// Table name: `entities`
    Table,
    /// Surrogate primary key every owned child relation references
    EntityKey,
    /// How many writes of the current state the row has taken, starting at
    /// one; with `entity_key`, the compare-and-set token an optimistic
    /// overwrite states. Not indexed, so its per-write update stays a
    /// heap-only-tuple update.
    WriteCount,
    /// Entity URN identifier, the external identity of the row, shared by
    /// the current state and the Temporal Evolution
    Id,
    /// Instant the current state was created (clause 4.8 `createdAt`), NULL
    /// while the Entity has no current state.
    CreatedAt,
    /// Instant the entity expires, when it has one (clause 4.22 `expiresAt`).
    ExpiresAt,
    /// Earliest `expiresAt` any attribute instance of the Entity carries, when
    /// one does (clause 4.22); what the attribute-expiry sweep ranges over.
    MinAttributeExpiresAt,
    /// Instant the entity was last modified (clause 4.8 `modifiedAt`); not
    /// indexed, so its per-write update stays a heap-only-tuple update. NULL
    /// while the Entity has no current state.
    ModifiedAt,
    /// Instant the Temporal Evolution began (clause 4.8 `createdAt`), never
    /// moved; NULL while the Entity has no Temporal Evolution
    EvolutionCreatedAt,
    /// Instant the Temporal Evolution was last extended (clause 4.8
    /// `modifiedAt`); NULL while the Entity has no Temporal Evolution
    EvolutionModifiedAt,
    /// The `entity_type_names` keys the Temporal Evolution is recorded under,
    /// which only grow (clauses 5.6.11.4 and 5.6.12.4); NULL while the Entity
    /// has no Temporal Evolution
    EvolutionTypeNameKeys,
}

/// The `entity_search` table - the indexed query axes of one current Entity.
///
/// Every index the planner uses to find entities is on this relation; the
/// write path rewrites its row only when one of these columns changed.
#[derive(Iden, Clone, Copy)]
pub enum EntitySearch {
    /// Table name: `entity_search`
    Table,
    /// Foreign key to `entities`, cascading on entity deletion; also the primary key
    EntityKey,
    /// The `xid8` of the transaction that last wrote the row
    WrittenXid,
    /// The clause 4.22 `expiresAt` of the hoisted default `location` instance,
    /// NULL when it carries none
    LocationExpiresAt,
    /// Array of `entity_type_names` keys, one per type the entity carries
    EntityTypeNameKeys,
    /// Array of `attribute_names` keys, one per attribute the Entity carries
    /// (clause 5.5.9 `attrs`)
    AttributeNameKeys,
    /// Array of `attribute_names` keys, one per attribute instance the Entity
    /// carries (clause 4.5.5), aligned with `attribute_instance_kinds`
    AttributeInstanceNameKeys,
    /// Array of attribute family codes, one per attribute instance, aligned
    /// with `attribute_instance_name_keys`
    AttributeInstanceKinds,
    /// Array of `ltree` scope paths an entity currently belongs to (clause 4.18).
    ScopePaths,
    /// The written spelling of each Scope (clause 4.18), aligned by position
    /// with `scope_paths`, which is what a read renders
    VerbatimScopePaths,
    /// The default `GeoProperty` geometry (clause 4.10).
    Location,
}

/// The `entity_type_members` table - one row per type one Entity carries in
/// its current state or its Temporal Evolution (clause 4.17), keyed by type
/// and then identifier.
///
/// A type selection walks one key range of it in identifier order, keeping
/// the rows of the side it reads, so a page of a type reads the rows it skips
/// and keeps and no Entity of another type.
#[derive(Iden, Clone, Copy)]
pub enum EntityTypeMembers {
    /// Table name: `entity_type_members`
    Table,
    /// Foreign key to `entities`, cascading on entity deletion
    EntityKey,
    /// The `entity_type_names` key of the type carried
    EntityTypeNameKey,
    /// Whether the current state carries the type
    IsCurrentMember,
    /// Whether the Temporal Evolution is recorded under the type
    IsEvolutionMember,
    /// The Entity's identifier, copied from `entities` so the walk is ordered by it
    Id,
}

/// The `current_attributes` table - one row per top-level attribute instance
/// of a current Entity, holding the instance whole.
///
/// The grain is the clause 4.5.5 dataset grain: an Entity, an attribute name
/// and an optional `datasetId`. Exactly one typed value column is populated on
/// a row, the one `value_kind` names; a String and a single Relationship object
/// each have two columns, picked by byte length
/// ([`StringValueColumn`](crate::string_value_column::StringValueColumn),
/// [`UriValueColumn`](crate::uri_value_column::UriValueColumn)).
/// The instance's sub-attributes are held on the same row in
/// `sub_attributes`, as elements of [`SubAttribute`].
///
/// A deleted attribute is an absent row rather than a stored marker, which is
/// why row presence alone answers an existence test.
#[derive(Iden, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurrentAttributes {
    /// Table name: `current_attributes`
    Table,
    /// The owning `entities` row; the statement-level check refuses a row
    /// naming none, and the Entity's deletion removes its rows
    EntityKey,
    /// Instant the instance was created (clause 4.8 `createdAt`)
    CreatedAt,
    /// Instant the instance was last modified (clause 4.8 `modifiedAt`)
    ModifiedAt,
    /// `attribute_names` key of the attribute
    AttributeNameKey,
    /// Written position of the instance among the Entity's attribute instances,
    /// contiguous from 0, which assembly restores the Entity's member order from
    Ordinal,
    /// Attribute family code of the instance
    AttributeKind,
    /// Value shape code, pinning which typed value column is populated
    ValueKind,
    /// `DateTime` value, when `value_kind` selects it
    ValueDateTime,
    /// The instance's clause 4.5.2 `observedAt`, NULL when it carries none
    ObservedAt,
    /// The instance's own clause 4.22 `expiresAt`, NULL when it never expires
    ExpiresAt,
    /// `entity_type_names` key of the datatype IRI a typed literal declares,
    /// beside the scalar column holding its value
    ValueDatatypeKey,
    /// `entity_type_names` key of the clause 4.5.2 `valueType`, NULL when the
    /// instance carries none
    ValueTypeKey,
    /// `cefact-units` identity of the instance's `unitCode` (clause 4.5.2)
    UnitCodeId,
    /// Boolean value, when `value_kind` selects it
    ValueBoolean,
    /// Optional dataset identifier (clause 4.5.5); NULL is the default instance
    DatasetId,
    /// Numeric value, when `value_kind` selects it
    ValueNumber,
    /// String value, when `value_kind` selects it
    ValueString,
    /// Language tag of a language-tagged String, beside `value_string` only
    ValueLanguage,
    /// Geometry value as written, when `value_kind` selects it
    ValueGeometry,
    /// The `ST_MakeValid` form of an invalid written geometry, NULL when the
    /// written one is valid; spatial predicates judge this form when present
    ValueGeometryValid,
    /// The `GeoJSON` members a geometry value carries beside its coordinates
    /// (`bbox` and foreign members), NULL when it carries none
    ValueGeometryMembers,
    /// Every value that is not one scalar, in the structured value encoding,
    /// when `value_kind` selects it
    ValueObject,
    /// URI value, when `value_kind` selects it
    ValueUri,
    /// String value too long for `value_string`'s B-tree, when `value_kind`
    /// selects String
    ValueLongText,
    /// URI value too long for `value_uri`'s B-tree, when `value_kind` selects
    /// URI
    ValueLongUri,
    /// `entity_type_names` keys of a Relationship's clause 4.5.3 `objectType`,
    /// in written order, NULL when it carries none
    ObjectTypeKeys,
    /// The clause 4.5.2.2 `entityIdSealed` and `entityTypeSealed` members,
    /// NULL when the instance carries neither
    ProofSeals,
    /// The instance's sub-attribute tree as an array of
    /// [`SubAttribute`] elements in written order, parents before
    /// children, NULL when the instance has no sub-attribute
    SubAttributes,
    /// The clause 4.9 equality keys of every scalar value of the sub-attribute
    /// tree, one per value, salted by the value's name path and kind; NULL
    /// when the tree holds no keyed value, never empty
    SubEqKeys,
}

/// The fields of the `sub_attribute` composite type - one
/// sub-attribute instance inside `current_attributes.sub_attributes`, in the
/// type's declaration order.
///
/// An element carries the members of a `current_attributes` row except the
/// length split: no B-tree keys on an element's strings or URIs, so each has
/// one field.
#[derive(Iden, Clone, Copy)]
pub enum SubAttribute {
    /// Type name: `sub_attribute`
    #[iden = "sub_attribute"]
    Type,
    /// 0 when the element is a sub-attribute of the top-level instance, else
    /// the 1-based array position of its parent element, which precedes it
    Parent,
    /// `attribute_names` key of the sub-attribute
    AttributeNameKey,
    /// Optional dataset identifier (clause 4.5.5); NULL is the default instance
    DatasetId,
    /// Attribute family code of the element
    AttributeKind,
    /// Value shape code, pinning which typed value field is populated
    ValueKind,
    /// Numeric value, when `value_kind` selects it
    ValueNumber,
    /// String value, when `value_kind` selects it
    ValueString,
    /// Boolean value, when `value_kind` selects it
    ValueBoolean,
    /// `DateTime` value, when `value_kind` selects it
    ValueDateTime,
    /// Geometry value, when `value_kind` selects it
    ValueGeometry,
    /// The `GeoJSON` members a geometry value carries beside its coordinates
    /// (`bbox` and foreign members), NULL when it carries none
    ValueGeometryMembers,
    /// Every value that is not one scalar, in the structured value encoding,
    /// when `value_kind` selects it
    ValueObject,
    /// URI value, when `value_kind` selects it
    ValueUri,
    /// `entity_type_names` key of the datatype IRI a typed literal declares,
    /// beside the scalar column holding its value
    ValueDatatypeKey,
    /// Language tag of a language-tagged String, beside `value_string` only
    ValueLanguage,
    /// `entity_type_names` key of the clause 4.5.2 `valueType`, NULL when the
    /// element carries none
    ValueTypeKey,
    /// `entity_type_names` keys of a Relationship's clause 4.5.3 `objectType`,
    /// in written order, NULL when it carries none
    ObjectTypeKeys,
    /// The clause 4.5.2.2 `entityIdSealed` and `entityTypeSealed` members,
    /// NULL when the element carries neither
    ProofSeals,
    /// `cefact-units` identity of the element's `unitCode` (clause 4.5.2)
    UnitCodeId,
    /// The element's clause 4.5.2 `observedAt`, NULL when it carries none
    ObservedAt,
    /// Instant the element was created (clause 4.8 `createdAt`)
    CreatedAt,
    /// Instant the element was last modified (clause 4.8 `modifiedAt`)
    ModifiedAt,
    /// The element's own clause 4.22 `expiresAt`, NULL when it never expires
    ExpiresAt,
}

/// Alias `e` for the entities table.
///
/// Shared by the context type predicate and the temporal stage-1 statement,
/// which both read the entities row under this alias.
#[derive(Iden, Clone, Copy)]
#[iden = "e"]
pub struct E;

#[cfg(test)]
mod tests {
    use crate::current::{CurrentAttributes, E, Entities, EntitySearch, EntityTypeMembers, SubAttribute};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn entities_table_name() {
        assert_eq!(Entities::Table.to_string(), "entities");
    }

    #[test]
    fn entities_column_names() {
        assert_eq!(Entities::EntityKey.to_string(), "entity_key");
        assert_eq!(Entities::WriteCount.to_string(), "write_count");
        assert_eq!(Entities::Id.to_string(), "id");
        assert_eq!(Entities::CreatedAt.to_string(), "created_at");
        assert_eq!(Entities::ExpiresAt.to_string(), "expires_at");
        assert_eq!(Entities::MinAttributeExpiresAt.to_string(), "min_attribute_expires_at");
        assert_eq!(Entities::EvolutionCreatedAt.to_string(), "evolution_created_at");
        assert_eq!(Entities::EvolutionModifiedAt.to_string(), "evolution_modified_at");
        assert_eq!(Entities::EvolutionTypeNameKeys.to_string(), "evolution_type_name_keys");
    }

    #[test]
    fn entity_search_names() {
        assert_eq!(EntitySearch::Table.to_string(), "entity_search");
        assert_eq!(EntitySearch::EntityKey.to_string(), "entity_key");
        assert_eq!(EntitySearch::WrittenXid.to_string(), "written_xid");
        assert_eq!(EntitySearch::LocationExpiresAt.to_string(), "location_expires_at");
        assert_eq!(EntitySearch::EntityTypeNameKeys.to_string(), "entity_type_name_keys");
        assert_eq!(EntitySearch::AttributeNameKeys.to_string(), "attribute_name_keys");
        assert_eq!(EntitySearch::AttributeInstanceNameKeys.to_string(), "attribute_instance_name_keys");
        assert_eq!(EntitySearch::AttributeInstanceKinds.to_string(), "attribute_instance_kinds");
        assert_eq!(EntitySearch::ScopePaths.to_string(), "scope_paths");
        assert_eq!(EntitySearch::Location.to_string(), "location");
    }

    #[test]
    fn entity_type_members_names() {
        assert_eq!(EntityTypeMembers::Table.to_string(), "entity_type_members");
        assert_eq!(EntityTypeMembers::EntityKey.to_string(), "entity_key");
        assert_eq!(EntityTypeMembers::EntityTypeNameKey.to_string(), "entity_type_name_key");
        assert_eq!(EntityTypeMembers::Id.to_string(), "id");
        assert_eq!(EntityTypeMembers::IsCurrentMember.to_string(), "is_current_member");
        assert_eq!(EntityTypeMembers::IsEvolutionMember.to_string(), "is_evolution_member");
    }

    #[test]
    fn entities_carries_the_modified_at_instant() {
        assert_eq!(Entities::ModifiedAt.to_string(), "modified_at");
    }

    #[test]
    fn entity_search_carries_verbatim_scope_paths() {
        assert_eq!(EntitySearch::VerbatimScopePaths.to_string(), "verbatim_scope_paths");
    }

    #[test]
    fn current_attributes_table_and_column_names() {
        let names = [
            CurrentAttributes::Table,
            CurrentAttributes::EntityKey,
            CurrentAttributes::CreatedAt,
            CurrentAttributes::ModifiedAt,
            CurrentAttributes::AttributeNameKey,
            CurrentAttributes::Ordinal,
            CurrentAttributes::AttributeKind,
            CurrentAttributes::ValueKind,
            CurrentAttributes::ValueDateTime,
            CurrentAttributes::ObservedAt,
            CurrentAttributes::ExpiresAt,
            CurrentAttributes::ValueDatatypeKey,
            CurrentAttributes::ValueTypeKey,
            CurrentAttributes::UnitCodeId,
            CurrentAttributes::ValueBoolean,
            CurrentAttributes::DatasetId,
            CurrentAttributes::ValueNumber,
            CurrentAttributes::ValueString,
            CurrentAttributes::ValueLanguage,
            CurrentAttributes::ValueGeometry,
            CurrentAttributes::ValueGeometryValid,
            CurrentAttributes::ValueGeometryMembers,
            CurrentAttributes::ValueObject,
            CurrentAttributes::ValueUri,
            CurrentAttributes::ValueLongText,
            CurrentAttributes::ValueLongUri,
            CurrentAttributes::ObjectTypeKeys,
            CurrentAttributes::ProofSeals,
            CurrentAttributes::SubAttributes,
            CurrentAttributes::SubEqKeys,
        ]
        .map(|name| name.to_string());
        assert_eq!(
            names,
            [
                "current_attributes",
                "entity_key",
                "created_at",
                "modified_at",
                "attribute_name_key",
                "ordinal",
                "attribute_kind",
                "value_kind",
                "value_date_time",
                "observed_at",
                "expires_at",
                "value_datatype_key",
                "value_type_key",
                "unit_code_id",
                "value_boolean",
                "dataset_id",
                "value_number",
                "value_string",
                "value_language",
                "value_geometry",
                "value_geometry_valid",
                "value_geometry_members",
                "value_object",
                "value_uri",
                "value_long_text",
                "value_long_uri",
                "object_type_keys",
                "proof_seals",
                "sub_attributes",
                "sub_eq_keys",
            ]
        );
    }

    #[test]
    fn sub_attribute_field_names_in_order() {
        let fields = [
            SubAttribute::Parent,
            SubAttribute::AttributeNameKey,
            SubAttribute::DatasetId,
            SubAttribute::AttributeKind,
            SubAttribute::ValueKind,
            SubAttribute::ValueNumber,
            SubAttribute::ValueString,
            SubAttribute::ValueBoolean,
            SubAttribute::ValueDateTime,
            SubAttribute::ValueGeometry,
            SubAttribute::ValueGeometryMembers,
            SubAttribute::ValueObject,
            SubAttribute::ValueUri,
            SubAttribute::ValueDatatypeKey,
            SubAttribute::ValueLanguage,
            SubAttribute::ValueTypeKey,
            SubAttribute::ObjectTypeKeys,
            SubAttribute::ProofSeals,
            SubAttribute::UnitCodeId,
            SubAttribute::ObservedAt,
            SubAttribute::CreatedAt,
            SubAttribute::ModifiedAt,
            SubAttribute::ExpiresAt,
        ]
        .map(|field| field.to_string());
        assert_eq!(SubAttribute::Type.to_string(), "sub_attribute");
        assert_eq!(
            fields,
            [
                "parent",
                "attribute_name_key",
                "dataset_id",
                "attribute_kind",
                "value_kind",
                "value_number",
                "value_string",
                "value_boolean",
                "value_date_time",
                "value_geometry",
                "value_geometry_members",
                "value_object",
                "value_uri",
                "value_datatype_key",
                "value_language",
                "value_type_key",
                "object_type_keys",
                "proof_seals",
                "unit_code_id",
                "observed_at",
                "created_at",
                "modified_at",
                "expires_at",
            ]
        );
    }

    #[test]
    fn the_entities_alias_is_e() {
        assert_eq!(E.to_string(), "e");
    }
}

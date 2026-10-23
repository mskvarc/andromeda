//! The shapes an attribute member is written in, and what every route that
//! reads one makes of each.
//!
//! A member reaches the compact model from a client's request (a creation and
//! a merge), from a peer's answer to a query or a temporal query (clause
//! 6.3.17), and from a notification another broker relays (clause 5.8.6). Each
//! route judges the same shape by the clauses that govern it, so one corpus
//! states every shape once with the verdict of every route, and each route's
//! tests read their verdict from it.

/// What a route makes of a member written in one shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The member is read into the typed model.
    Typed,
    /// The member is refused: a client is answered `BadRequestData` (clause
    /// 5.5.4), a peer's entity is dropped with a clause 6.3.17 warning.
    Refused,
    /// The member is read, the member it names left unread: clause 5.2.2 has
    /// an implementation ignore a system member a Context Producer supplies.
    Ignored(&'static str),
    /// The route never reads a member written in this shape.
    NotApplicable,
}

/// One shape of attribute member, written under the attribute `attr`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapeCase {
    /// The shape, as the inventory names it.
    pub name: &'static str,
    /// The JSON text of the member.
    pub member: &'static str,
    /// The user `@context` the member is written under, as JSON text, or
    /// `None` for the Core `@context` alone.
    pub context: Option<&'static str>,
    /// What a creation makes of it (clause 5.6.1).
    pub request: Verdict,
    /// What a merge makes of it (clause 5.5.12).
    pub merge: Verdict,
    /// What a peer's answer to a query makes of it (clause 6.3.17).
    pub peer_query: Verdict,
    /// What a peer's answer to a temporal query makes of it, the member being
    /// the one instance of the attribute's history.
    pub peer_temporal: Verdict,
    /// What a relayed notification makes of it (clause 5.8.6).
    pub relay: Verdict,
}

impl ShapeCase {
    /// A shape every route but the temporal one judges `verdict`.
    const fn everywhere(name: &'static str, member: &'static str, verdict: Verdict) -> ShapeCase {
        ShapeCase {
            name,
            member,
            context: None,
            request: verdict,
            merge: verdict,
            peer_query: verdict,
            peer_temporal: Verdict::NotApplicable,
            relay: verdict,
        }
    }
}

/// The user `@context` aliasing `@type`, `@id` and `ngsi-ld:hasValue`.
pub const ALIASING_CONTEXT: &str = r#"{"kind": "@type", "ident": "@id", "v": "https://uri.etsi.org/ngsi-ld/hasValue"}"#;

/// Clause 4.5.2.3: a bare scalar is a concise Property.
pub const S1_BARE_SCALAR: ShapeCase = ShapeCase::everywhere("S1 bare scalar", "5", Verdict::Typed);

/// Clause 4.5.2.3: an array of values is a concise Property.
pub const S2_ARRAY_OF_VALUES: ShapeCase = ShapeCase::everywhere("S2 array of values", "[1, 2, 3]", Verdict::Typed);

/// Clause 4.5.2.3: the empty array is a concise Property too.
pub const S2_EMPTY_ARRAY: ShapeCase = ShapeCase::everywhere("S2 empty array", "[]", Verdict::Typed);

/// Clause 4.5.2.3: an object naming no type is a concise Property.
pub const S3_TYPELESS_OBJECT: ShapeCase = ShapeCase::everywhere("S3 typeless object", r#"{"street": "Main", "no": 1}"#, Verdict::Typed);

/// Clause 4.5.2.3: a typeless instance carrying `value`.
pub const S4_TYPELESS_VALUE: ShapeCase = ShapeCase::everywhere("S4 typeless value", r#"{"value": 21, "unitCode": "CEL"}"#, Verdict::Typed);

/// Clause 4.7.3: a typeless instance whose value is a geometry.
pub const S4_TYPELESS_GEOMETRY_VALUE: ShapeCase = ShapeCase::everywhere(
    "S4 typeless geometry value",
    r#"{"value": {"type": "Point", "coordinates": [13.4, 52.5]}}"#,
    Verdict::Typed,
);

/// Clause 4.5.3.3: a typeless instance carrying `object`.
pub const S5_TYPELESS_OBJECT_MEMBER: ShapeCase = ShapeCase::everywhere("S5 typeless object", r#"{"object": "urn:ngsi-ld:Owner:1"}"#, Verdict::Typed);

/// Clause 4.5.18.3: a typeless instance carrying `languageMap`.
pub const S5_TYPELESS_LANGUAGE_MAP: ShapeCase =
    ShapeCase::everywhere("S5 typeless languageMap", r#"{"languageMap": {"en": "Tower", "de": "Turm"}}"#, Verdict::Typed);

/// Clause 4.5.20.3: a typeless instance carrying `vocab`.
pub const S5_TYPELESS_VOCAB: ShapeCase = ShapeCase::everywhere("S5 typeless vocab", r#"{"vocab": "Tower"}"#, Verdict::Typed);

/// Clause 4.5.21.3: a typeless instance carrying `valueList`.
pub const S5_TYPELESS_VALUE_LIST: ShapeCase = ShapeCase::everywhere("S5 typeless valueList", r#"{"valueList": [1, 2]}"#, Verdict::Typed);

/// Clause 4.5.22.3: a typeless instance carrying `objectList`.
pub const S5_TYPELESS_OBJECT_LIST: ShapeCase = ShapeCase::everywhere("S5 typeless objectList", r#"{"objectList": ["urn:ngsi-ld:Owner:1"]}"#, Verdict::Typed);

/// Clause 4.5.24.3: a typeless instance carrying `json`.
pub const S5_TYPELESS_JSON: ShapeCase = ShapeCase::everywhere("S5 typeless json", r#"{"json": {"a": [1, {"b": null}]}}"#, Verdict::Typed);

/// Clause 4.7.3: a bare geometry is a concise `GeoProperty`.
pub const S6_BARE_GEOMETRY: ShapeCase = ShapeCase::everywhere("S6 bare geometry", r#"{"type": "Point", "coordinates": [13.4, 52.5]}"#, Verdict::Typed);

/// Clause 4.5.5: a multi-attribute one of whose instances is concise.
pub const S7_ARRAY_WITH_A_CONCISE_INSTANCE: ShapeCase = ShapeCase::everywhere(
    "S7 array with a concise instance",
    r#"[{"value": 1, "datasetId": "urn:ngsi-ld:Dataset:a"}, {"type": "Property", "value": 2}]"#,
    Verdict::Typed,
);

/// Clause 4.7.2: a geometry may be written as the JSON string encoding it.
pub const S8_GEOMETRY_AS_A_STRING: ShapeCase = ShapeCase::everywhere(
    "S8 geometry as a string",
    r#"{"type": "GeoProperty", "value": "{\"type\":\"Point\",\"coordinates\":[1,2]}"}"#,
    Verdict::Typed,
);

/// Clause 4.7.2: a string that encodes no geometry is no `GeoProperty` value.
pub const S8_STRING_ENCODING_NO_GEOMETRY: ShapeCase = ShapeCase::everywhere(
    "S8 string encoding no geometry",
    r#"{"type": "GeoProperty", "value": "not a geometry"}"#,
    Verdict::Refused,
);

/// Clause 5.5.4 refuses the NGSI-LD Null outside a merge, which removes the
/// member (clause 5.5.12), and a notification reports a deleted attribute so
/// (clause 5.8.6).
pub const S9_BARE_NULL: ShapeCase = ShapeCase {
    merge: Verdict::Typed,
    relay: Verdict::Typed,
    ..ShapeCase::everywhere("S9 bare NGSI-LD Null", r#""urn:ngsi-ld:null""#, Verdict::Refused)
};

/// Clause 4.5.0 uses the NGSI-LD Null as the value of a deleted `GeoProperty`
/// in a notification and a temporal representation; clause 5.5.4 refuses it
/// elsewhere outside a merge.
pub const S10_GEOPROPERTY_NULL_VALUE: ShapeCase = ShapeCase {
    merge: Verdict::Typed,
    peer_temporal: Verdict::Typed,
    relay: Verdict::Typed,
    ..ShapeCase::everywhere(
        "S10 GeoProperty Null value",
        r#"{"type": "GeoProperty", "value": "urn:ngsi-ld:null"}"#,
        Verdict::Refused,
    )
};

/// Clause 5.5.12: a merge removes the `unitCode` written as the NGSI-LD Null.
pub const S11_NULL_UNIT_CODE: ShapeCase = ShapeCase {
    merge: Verdict::Typed,
    ..ShapeCase::everywhere(
        "S11 Null unitCode",
        r#"{"type": "Property", "value": 1, "unitCode": "urn:ngsi-ld:null"}"#,
        Verdict::Refused,
    )
};

/// Clause 5.5.12: a merge removes the `observedAt` written as the NGSI-LD Null.
pub const S11_NULL_OBSERVED_AT: ShapeCase = ShapeCase {
    merge: Verdict::Typed,
    ..ShapeCase::everywhere(
        "S11 Null observedAt",
        r#"{"type": "Property", "value": 1, "observedAt": "urn:ngsi-ld:null"}"#,
        Verdict::Refused,
    )
};

/// Clause 5.5.12, last bullet: a `datasetId` is never removed by a merge.
pub const S11_NULL_DATASET_ID: ShapeCase = ShapeCase::everywhere(
    "S11 Null datasetId",
    r#"{"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:null"}"#,
    Verdict::Refused,
);

/// Clauses 4.5.0 and 5.2.1: no member admits JSON `null`.
pub const S12_JSON_NULL: ShapeCase = ShapeCase::everywhere("S12 JSON null", "null", Verdict::Refused);

/// Clause 4.5.2.2: a type naming no attribute type.
pub const S13_UNKNOWN_TYPE: ShapeCase = ShapeCase::everywhere("S13 unknown type", r#"{"type": "Propertyy", "value": 1}"#, Verdict::Refused);

/// Clause 4.7.3: a geometry type without its coordinates.
pub const S14_GEOMETRY_WITHOUT_COORDINATES: ShapeCase = ShapeCase::everywhere("S14 geometry without coordinates", r#"{"type": "Point"}"#, Verdict::Refused);

/// Clause 4.5.2.2 requires the value member; clause 5.5.12 lets a merge keep
/// the value held.
pub const S15_INSTANCE_WITHOUT_ITS_VALUE: ShapeCase = ShapeCase {
    merge: Verdict::Typed,
    peer_temporal: Verdict::Refused,
    ..ShapeCase::everywhere("S15 instance without its value", r#"{"type": "Property"}"#, Verdict::Refused)
};

/// Clause 4.5.3.2: an `object` is a URI.
pub const S16_UNDECODABLE_FAMILY_VALUE: ShapeCase = ShapeCase::everywhere(
    "S16 undecodable family value",
    r#"{"type": "Relationship", "object": "not a uri"}"#,
    Verdict::Refused,
);

/// Clause 4.8: an `observedAt` is a `DateTime`.
pub const S17_UNDECODABLE_ENVELOPE_MEMBER: ShapeCase = ShapeCase::everywhere(
    "S17 undecodable envelope member",
    r#"{"type": "Property", "value": 1, "observedAt": "yesterday"}"#,
    Verdict::Refused,
);

/// Clause 5.2.2: a producer's system member is ignored; a peer's own system
/// members are `DateTime`s (clause 4.8).
pub const S18_MALFORMED_SYSTEM_MEMBER: ShapeCase = ShapeCase {
    request: Verdict::Ignored("createdAt"),
    merge: Verdict::Ignored("createdAt"),
    ..ShapeCase::everywhere(
        "S18 malformed system member",
        r#"{"type": "Property", "value": 1, "createdAt": "not a time"}"#,
        Verdict::Refused,
    )
};

/// Clause 4.6.2: a sub-attribute is named by an attribute name.
pub const S19_BAD_SUB_ATTRIBUTE_NAME: ShapeCase = ShapeCase::everywhere(
    "S19 bad sub-attribute name",
    r#"{"type": "Property", "value": 1, "bad name": {"type": "Property", "value": 2}}"#,
    Verdict::Refused,
);

/// Clause 4.5.5: a multi-attribute is an array of instances.
pub const S20_MIXED_ARRAY: ShapeCase = ShapeCase::everywhere("S20 mixed array", r#"[{"type": "Property", "value": 1}, 1]"#, Verdict::Refused);

/// Clause 4.5.2.3: a concise instance infers one family.
pub const S21_SEVERAL_FAMILY_VALUE_MEMBERS: ShapeCase = ShapeCase::everywhere(
    "S21 several family value members",
    r#"{"object": "urn:ngsi-ld:Owner:1", "vocab": "Tower"}"#,
    Verdict::Refused,
);

/// Clause 5.5.7: no JSON-LD keyword inside a geometry.
pub const S22_GEOMETRY_WITH_A_KEYWORD: ShapeCase = ShapeCase::everywhere(
    "S22 geometry with a JSON-LD keyword",
    r#"{"type": "GeoProperty", "value": {"@id": "urn:ngsi-ld:Geometry:1", "type": "Point", "coordinates": [13.4, 52.5]}}"#,
    Verdict::Refused,
);

/// Clause 4.4: `@type` stands for `type`.
pub const S23_KEYWORD_SPELLINGS: ShapeCase = ShapeCase {
    peer_temporal: Verdict::Typed,
    ..ShapeCase::everywhere("S23a keyword spellings", r#"{"@type": "Property", "value": 1}"#, Verdict::Typed)
};

/// Clause 5.5.7: a user `@context` may alias `@type` and `ngsi-ld:hasValue`.
pub const S23_USER_ALIASES: ShapeCase = ShapeCase {
    context: Some(ALIASING_CONTEXT),
    ..ShapeCase::everywhere("S23b user aliases", r#"{"kind": "Property", "v": 1}"#, Verdict::Typed)
};

/// Invoke the macro `$route` once with every shape, each as
/// `test_name => path::TO_THE_CASE`, so every route names its test of a shape
/// alike and none copies the list.
///
/// `$route` expands one test per pair; the name says the shape in words.
#[macro_export]
macro_rules! for_each_attribute_member_shape {
    ($route:ident) => {
        $route! {
            a_bare_scalar => $crate::attribute_member_shape::S1_BARE_SCALAR,
            an_array_of_values => $crate::attribute_member_shape::S2_ARRAY_OF_VALUES,
            an_empty_array => $crate::attribute_member_shape::S2_EMPTY_ARRAY,
            a_typeless_object => $crate::attribute_member_shape::S3_TYPELESS_OBJECT,
            a_typeless_instance_with_a_value => $crate::attribute_member_shape::S4_TYPELESS_VALUE,
            a_typeless_instance_with_a_geometry_value => $crate::attribute_member_shape::S4_TYPELESS_GEOMETRY_VALUE,
            a_typeless_instance_with_an_object => $crate::attribute_member_shape::S5_TYPELESS_OBJECT_MEMBER,
            a_typeless_instance_with_a_language_map => $crate::attribute_member_shape::S5_TYPELESS_LANGUAGE_MAP,
            a_typeless_instance_with_a_vocab => $crate::attribute_member_shape::S5_TYPELESS_VOCAB,
            a_typeless_instance_with_a_value_list => $crate::attribute_member_shape::S5_TYPELESS_VALUE_LIST,
            a_typeless_instance_with_an_object_list => $crate::attribute_member_shape::S5_TYPELESS_OBJECT_LIST,
            a_typeless_instance_with_a_json_value => $crate::attribute_member_shape::S5_TYPELESS_JSON,
            a_bare_geometry => $crate::attribute_member_shape::S6_BARE_GEOMETRY,
            an_array_with_a_concise_instance => $crate::attribute_member_shape::S7_ARRAY_WITH_A_CONCISE_INSTANCE,
            a_geometry_written_as_a_string => $crate::attribute_member_shape::S8_GEOMETRY_AS_A_STRING,
            a_string_encoding_no_geometry => $crate::attribute_member_shape::S8_STRING_ENCODING_NO_GEOMETRY,
            a_bare_ngsi_ld_null => $crate::attribute_member_shape::S9_BARE_NULL,
            a_geoproperty_whose_value_is_the_null => $crate::attribute_member_shape::S10_GEOPROPERTY_NULL_VALUE,
            a_null_unit_code => $crate::attribute_member_shape::S11_NULL_UNIT_CODE,
            a_null_observed_at => $crate::attribute_member_shape::S11_NULL_OBSERVED_AT,
            a_null_dataset_id => $crate::attribute_member_shape::S11_NULL_DATASET_ID,
            a_json_null => $crate::attribute_member_shape::S12_JSON_NULL,
            an_unknown_type => $crate::attribute_member_shape::S13_UNKNOWN_TYPE,
            a_geometry_without_coordinates => $crate::attribute_member_shape::S14_GEOMETRY_WITHOUT_COORDINATES,
            an_instance_without_its_value => $crate::attribute_member_shape::S15_INSTANCE_WITHOUT_ITS_VALUE,
            an_undecodable_family_value => $crate::attribute_member_shape::S16_UNDECODABLE_FAMILY_VALUE,
            an_undecodable_envelope_member => $crate::attribute_member_shape::S17_UNDECODABLE_ENVELOPE_MEMBER,
            a_malformed_system_member => $crate::attribute_member_shape::S18_MALFORMED_SYSTEM_MEMBER,
            a_bad_sub_attribute_name => $crate::attribute_member_shape::S19_BAD_SUB_ATTRIBUTE_NAME,
            a_mixed_array => $crate::attribute_member_shape::S20_MIXED_ARRAY,
            several_family_value_members => $crate::attribute_member_shape::S21_SEVERAL_FAMILY_VALUE_MEMBERS,
            a_geometry_with_a_json_ld_keyword => $crate::attribute_member_shape::S22_GEOMETRY_WITH_A_KEYWORD,
            an_instance_spelled_with_keywords => $crate::attribute_member_shape::S23_KEYWORD_SPELLINGS,
            an_instance_spelled_with_user_aliases => $crate::attribute_member_shape::S23_USER_ALIASES,
        }
    };
}

/// Every shape, in the inventory's order.
pub const ALL_SHAPES: &[ShapeCase] = &[
    S1_BARE_SCALAR,
    S2_ARRAY_OF_VALUES,
    S2_EMPTY_ARRAY,
    S3_TYPELESS_OBJECT,
    S4_TYPELESS_VALUE,
    S4_TYPELESS_GEOMETRY_VALUE,
    S5_TYPELESS_OBJECT_MEMBER,
    S5_TYPELESS_LANGUAGE_MAP,
    S5_TYPELESS_VOCAB,
    S5_TYPELESS_VALUE_LIST,
    S5_TYPELESS_OBJECT_LIST,
    S5_TYPELESS_JSON,
    S6_BARE_GEOMETRY,
    S7_ARRAY_WITH_A_CONCISE_INSTANCE,
    S8_GEOMETRY_AS_A_STRING,
    S8_STRING_ENCODING_NO_GEOMETRY,
    S9_BARE_NULL,
    S10_GEOPROPERTY_NULL_VALUE,
    S11_NULL_UNIT_CODE,
    S11_NULL_OBSERVED_AT,
    S11_NULL_DATASET_ID,
    S12_JSON_NULL,
    S13_UNKNOWN_TYPE,
    S14_GEOMETRY_WITHOUT_COORDINATES,
    S15_INSTANCE_WITHOUT_ITS_VALUE,
    S16_UNDECODABLE_FAMILY_VALUE,
    S17_UNDECODABLE_ENVELOPE_MEMBER,
    S18_MALFORMED_SYSTEM_MEMBER,
    S19_BAD_SUB_ATTRIBUTE_NAME,
    S20_MIXED_ARRAY,
    S21_SEVERAL_FAMILY_VALUE_MEMBERS,
    S22_GEOMETRY_WITH_A_KEYWORD,
    S23_KEYWORD_SPELLINGS,
    S23_USER_ALIASES,
];

#[cfg(test)]
mod tests {
    use crate::attribute_member_shape::{ALL_SHAPES, S9_BARE_NULL, S18_MALFORMED_SYSTEM_MEMBER, S23_USER_ALIASES, Verdict};

    /// A case overriding one route keeps every other route's verdict.
    #[test]
    fn an_override_keeps_the_other_verdicts() {
        pretty_assertions::assert_eq!(S9_BARE_NULL.request, Verdict::Refused);
        pretty_assertions::assert_eq!(S9_BARE_NULL.merge, Verdict::Typed);
        pretty_assertions::assert_eq!(S9_BARE_NULL.peer_temporal, Verdict::NotApplicable);
        pretty_assertions::assert_eq!(S18_MALFORMED_SYSTEM_MEMBER.request, Verdict::Ignored("createdAt"));
        pretty_assertions::assert_eq!(S18_MALFORMED_SYSTEM_MEMBER.relay, Verdict::Refused);
    }

    /// Every member is JSON text, the aliasing context included.
    #[test]
    fn every_member_is_json() {
        for case in ALL_SHAPES {
            assert!(serde_json::from_str::<serde_json::Value>(case.member).is_ok(), "{}", case.name);
        }
        assert!(serde_json::from_str::<serde_json::Value>(S23_USER_ALIASES.context.unwrap_or_default()).is_ok());
    }
}

//! The bind vectors the rows of many Entities travel in, one per staged column
//! of the `instances` staging
//! ([`instances_cte`](crate::current_attribute::staging::instances_cte)).
//!
//! Everything that turns a row into a column happens on the way in here: the
//! length split a String or a URI makes, the EWKB of a geometry, the array
//! literal of the `objectType` keys and the `sub_attribute_tree` each row's
//! elements travel in.

use crate::current_attribute::{
    row::{CurrentAttributeRow, InstanceMembers},
    stored_json::StoredJson,
    stored_value::StoredValue,
    sub_attribute::{SubAttributeRecord, SubAttributeTree},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, value_kind::ValueKind};
use andromeda_postgresql_encoding::{
    array_literal::IntegerArrayLiteral,
    ewkb::{EwkbError, encode},
};
use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
use andromeda_postgresql_schema::current::CurrentAttributes;
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde_json::value::RawValue;
use sqlx::{Postgres, postgres::PgArguments, query::QueryAs, types::Json};

/// The value columns one stored value occupies, exactly one of them set.
#[derive(Default)]
struct ValueColumns {
    /// `value_number`.
    number: Option<BigDecimal>,
    /// `value_string`.
    string: Option<String>,
    /// `value_long_text`.
    long_text: Option<String>,
    /// `value_boolean`.
    boolean: Option<bool>,
    /// `value_date_time`.
    date_time: Option<DateTime<Utc>>,
    /// `value_geometry`, as EWKB.
    geometry: Option<Vec<u8>>,
    /// `value_geometry_members`.
    geometry_members: Option<Json<Box<RawValue>>>,
    /// `value_object`.
    object: Option<Json<Box<RawValue>>>,
    /// `value_uri`.
    uri: Option<String>,
    /// `value_long_uri`.
    long_uri: Option<String>,
}

impl ValueColumns {
    /// The columns `value` occupies, a String or a URI past its B-tree bound
    /// in its overflow column.
    fn of(value: StoredValue) -> Result<ValueColumns, EwkbError> {
        let column = value.row_column();
        let mut columns = ValueColumns::default();
        match value {
            // clone: the bind owns the decimal it encodes, and the number gives up none.
            StoredValue::Number { number, .. } => columns.number = Some(number.decimal().clone()),
            StoredValue::String { text, .. } if column == CurrentAttributes::ValueLongText => columns.long_text = Some(text),
            StoredValue::String { text, .. } => columns.string = Some(text),
            StoredValue::Boolean { flag, .. } => columns.boolean = Some(flag),
            StoredValue::DateTime { instant, .. } => columns.date_time = Some(instant),
            StoredValue::Geometry { form, members } => {
                columns.geometry = Some(encode(&form)?.to_vec());
                columns.geometry_members = members.map(StoredJson::into_json);
            }
            StoredValue::Object(object) => columns.object = Some(object.into_json()),
            StoredValue::Uri(uri) if column == CurrentAttributes::ValueLongUri => columns.long_uri = Some(String::from(uri)),
            StoredValue::Uri(uri) => columns.uri = Some(String::from(uri)),
        }
        Ok(columns)
    }
}

/// The rows of many Entities, one vector per staged column, in bind order.
#[derive(Debug, Default)]
pub struct StagedRows<'a> {
    /// The identifier of the Entity each row belongs to.
    entity_ids: Vec<&'a str>,
    /// `attribute_name_key`.
    attribute_name_keys: Vec<i32>,
    /// `dataset_id`.
    dataset_ids: Vec<Option<String>>,
    /// `ordinal`.
    ordinals: Vec<i32>,
    /// `attribute_kind`.
    attribute_kinds: Vec<i16>,
    /// `value_kind`.
    value_kinds: Vec<i16>,
    /// `value_number`.
    value_numbers: Vec<Option<BigDecimal>>,
    /// `value_string`.
    value_strings: Vec<Option<String>>,
    /// `value_long_text`.
    value_long_texts: Vec<Option<String>>,
    /// `value_boolean`.
    value_booleans: Vec<Option<bool>>,
    /// `value_date_time`.
    value_date_times: Vec<Option<DateTime<Utc>>>,
    /// `value_geometry`, as EWKB.
    value_geometries: Vec<Option<Vec<u8>>>,
    /// `value_geometry_members`.
    value_geometry_members: Vec<Option<Json<Box<RawValue>>>>,
    /// `value_object`.
    value_objects: Vec<Option<Json<Box<RawValue>>>>,
    /// `value_uri`.
    value_uris: Vec<Option<String>>,
    /// `value_long_uri`.
    value_long_uris: Vec<Option<String>>,
    /// `value_datatype_key`.
    value_datatype_keys: Vec<Option<i32>>,
    /// `value_language`.
    value_languages: Vec<Option<String>>,
    /// `value_type_key`.
    value_type_keys: Vec<Option<i32>>,
    /// `object_type_keys`, as array literals, NULL when the list is empty.
    object_type_keys: Vec<Option<IntegerArrayLiteral>>,
    /// `unit_code_id`.
    unit_code_ids: Vec<Option<i16>>,
    /// `observed_at`.
    observed_ats: Vec<Option<DateTime<Utc>>>,
    /// `created_at`.
    created_ats: Vec<DateTime<Utc>>,
    /// `modified_at`.
    modified_ats: Vec<DateTime<Utc>>,
    /// `expires_at`.
    expires_ats: Vec<Option<DateTime<Utc>>>,
    /// `proof_seals`.
    proof_seals: Vec<Option<Json<Box<RawValue>>>>,
    /// `sub_attributes`, NULL when the instance has none.
    sub_attributes: Vec<Option<SubAttributeTree>>,
}

impl<'a> StagedRows<'a> {
    /// Stage the rows of the Entity `entity`.
    ///
    /// # Errors
    ///
    /// Returns [`EwkbError`] when a geometry, of a row or of an element,
    /// cannot be encoded as EWKB.
    pub fn push(&mut self, entity: &'a Identifier, rows: Vec<CurrentAttributeRow>) -> Result<(), EwkbError> {
        for row in rows {
            let CurrentAttributeRow {
                ordinal,
                members,
                created_at,
                modified_at,
                sub_attributes,
            } = row;
            let InstanceMembers {
                attribute_name_key,
                dataset_id,
                attribute_kind,
                value,
                value_type_key,
                object_type_keys,
                unit_code_id,
                observed_at,
                expires_at,
                proof_seals,
            } = members;
            let datatype_key = value.datatype_key().map(EntityTypeNameKey::get);
            let language = value.language().map(|language| language.as_str().to_owned());
            let kind = ValueKind::code(value.kind());
            let values = ValueColumns::of(value)?;
            let elements = sub_attributes
                .into_iter()
                .map(SubAttributeRecord::try_from)
                .collect::<Result<Vec<_>, EwkbError>>()?;
            self.entity_ids.push(entity.as_str());
            self.attribute_name_keys.push(attribute_name_key.get());
            self.dataset_ids.push(dataset_id.map(String::from));
            self.ordinals.push(ordinal);
            self.attribute_kinds.push(AttributeKind::code(attribute_kind));
            self.value_kinds.push(kind);
            self.value_numbers.push(values.number);
            self.value_strings.push(values.string);
            self.value_long_texts.push(values.long_text);
            self.value_booleans.push(values.boolean);
            self.value_date_times.push(values.date_time);
            self.value_geometries.push(values.geometry);
            self.value_geometry_members.push(values.geometry_members);
            self.value_objects.push(values.object);
            self.value_uris.push(values.uri);
            self.value_long_uris.push(values.long_uri);
            self.value_datatype_keys.push(datatype_key);
            self.value_languages.push(language);
            self.value_type_keys.push(value_type_key.map(EntityTypeNameKey::get));
            self.object_type_keys
                .push((!object_type_keys.is_empty()).then(|| IntegerArrayLiteral::of(object_type_keys.iter().map(|key| i64::from(key.get())))));
            self.unit_code_ids.push(unit_code_id.map(UnitCodeId::get));
            self.observed_ats.push(observed_at);
            self.created_ats.push(created_at);
            self.modified_ats.push(modified_at);
            self.expires_ats.push(expires_at);
            self.proof_seals.push(proof_seals.map(StoredJson::into_json));
            self.sub_attributes.push((!elements.is_empty()).then_some(SubAttributeTree { elements }));
        }
        Ok(())
    }

    /// The number of rows staged.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entity_ids.len()
    }

    /// Whether no row is staged.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entity_ids.is_empty()
    }

    /// Bind every staged column to `query`, in the order
    /// [`instances_cte`](crate::current_attribute::staging::instances_cte)
    /// reads them.
    pub(crate) fn bind_to<'q, O>(self, query: QueryAs<'q, Postgres, O, PgArguments>) -> QueryAs<'q, Postgres, O, PgArguments>
    where
        'a: 'q,
    {
        let object_type_keys: Vec<Option<String>> = self
            .object_type_keys
            .iter()
            .map(|keys| keys.as_ref().map(|keys| keys.as_str().to_owned()))
            .collect();
        query
            .bind(self.entity_ids)
            .bind(self.attribute_name_keys)
            .bind(self.dataset_ids)
            .bind(self.ordinals)
            .bind(self.attribute_kinds)
            .bind(self.value_kinds)
            .bind(self.value_numbers)
            .bind(self.value_strings)
            .bind(self.value_long_texts)
            .bind(self.value_booleans)
            .bind(self.value_date_times)
            .bind(self.value_geometries)
            .bind(self.value_geometry_members)
            .bind(self.value_objects)
            .bind(self.value_uris)
            .bind(self.value_long_uris)
            .bind(self.value_datatype_keys)
            .bind(self.value_languages)
            .bind(self.value_type_keys)
            .bind(object_type_keys)
            .bind(self.unit_code_ids)
            .bind(self.observed_ats)
            .bind(self.created_ats)
            .bind(self.modified_ats)
            .bind(self.expires_ats)
            .bind(self.proof_seals)
            .bind(self.sub_attributes)
    }
}

#[cfg(test)]
mod tests {
    use crate::current_attribute::{
        flatten::flatten,
        round_trip_tests::{committed, dictionary, document, ex, family, ngsi, property, with},
        staged_rows::StagedRows,
    };
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use serde_json::{Value, json};

    fn entity_id() -> Identifier {
        "urn:ngsi-ld:Building:1".parse().expect("a valid identifier")
    }

    /// The rows of `state`, staged.
    fn staged<'a>(id: &'a Identifier, state: &EntityState) -> StagedRows<'a> {
        let (keys, _) = dictionary(state);
        let mut rows = StagedRows::default();
        rows.push(id, flatten(state.attributes(), &keys).expect("a committed state flattens"))
            .expect("every geometry encodes");
        rows
    }

    /// A Property holding `sub_attributes`.
    fn holding(value: i64, sub_attributes: &Value) -> Value {
        with(property(&json!([{"@value": value}])), sub_attributes)
    }

    /// Every depth of an instance travels inside its own row, so an entity
    /// nested three levels deep beside a multi-object Relationship stages one
    /// row per top-level instance and no row for any sub-attribute.
    #[test]
    fn a_create_stages_every_depth_inside_its_rows() {
        let level3 = holding(3, &json!({}));
        let level2 = holding(2, &json!({ex("c"): [level3]}));
        let level1 = holding(1, &json!({ex("b"): [level2]}));
        let state = committed(&document(&json!({
            ex("a"): [level1],
            ex("rel"): [{"@type": family("Relationship"), ngsi("hasObject"): [{"@id": "urn:b"}, {"@id": "urn:a"}]}]
        })));
        let id = entity_id();
        let rows = staged(&id, &state);
        pretty_assertions::assert_eq!(rows.len(), 2);
        pretty_assertions::assert_eq!(rows.entity_ids, vec!["urn:ngsi-ld:Building:1"; 2]);
        let tree = rows.sub_attributes[0].as_ref().expect("the nested instance carries its tree");
        pretty_assertions::assert_eq!(tree.elements.iter().map(|element| element.parent).collect::<Vec<_>>(), vec![0, 1]);
        assert!(rows.sub_attributes[1].is_none(), "an instance without sub-attributes stages no tree");
        assert!(rows.value_uris[1].is_none());
        pretty_assertions::assert_eq!(
            rows.value_objects[1].as_ref().map(|object| object.0.get()),
            Some(r#"["urn:b","urn:a"]"#),
            "the objects of a multi-object Relationship stay in written order"
        );
    }

    /// A String or a URI past its B-tree bound stages in its overflow column,
    /// under the same value kind.
    #[test]
    fn long_strings_and_uris_stage_in_their_overflow_columns() {
        let long_text = "x".repeat(2049);
        let long_uri = format!("urn:ngsi-ld:{}", "y".repeat(1100));
        // `json!` orders the attributes by name: `far`, `note`, `short`.
        let state = committed(&document(&json!({
            ex("note"): [property(&json!([{"@value": long_text}]))],
            ex("short"): [property(&json!([{"@value": "short"}]))],
            ex("far"): [{"@type": family("Relationship"), ngsi("hasObject"): [{"@id": long_uri}]}]
        })));
        let id = entity_id();
        let rows = staged(&id, &state);
        pretty_assertions::assert_eq!(rows.value_long_uris[0].as_deref(), Some(long_uri.as_str()));
        assert!(rows.value_uris[0].is_none());
        pretty_assertions::assert_eq!(rows.value_long_texts[1].as_deref().map(str::len), Some(2049));
        assert!(rows.value_strings[1].is_none());
        pretty_assertions::assert_eq!(rows.value_strings[2].as_deref(), Some("short"));
        pretty_assertions::assert_eq!(rows.value_kinds, vec![7, 2, 2]);
    }

    /// The `objectType` keys travel as one array literal per row, NULL when the
    /// list is empty.
    #[test]
    fn object_type_keys_stage_as_an_array_literal() {
        let state = committed(&document(&json!({
            ex("owner"): [{
                "@type": family("Relationship"),
                ngsi("hasObject"): [{"@id": "urn:a"}],
                ngsi("hasObjectType"): [{"@id": ex("Person")}, {"@id": ex("Agent")}]
            }],
            ex("plain"): [{"@type": family("Relationship"), ngsi("hasObject"): [{"@id": "urn:b"}]}]
        })));
        let id = entity_id();
        let rows = staged(&id, &state);
        let literal = rows.object_type_keys[0].as_ref().map(|keys| keys.as_str().to_owned());
        assert!(
            literal.as_deref().is_some_and(|keys| keys.starts_with('{') && keys.matches(',').count() == 1),
            "{literal:?}"
        );
        assert!(rows.object_type_keys[1].is_none());
    }
}

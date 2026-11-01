//! The family value an instance's stored columns hold.
//!
//! The attribute kind names the family, and each family reads its value from
//! the columns flattening puts it in: a scalar from its own column, anything
//! else from `value_object` through the structured value codec. A value in a
//! column its family never uses, or a member its family never defines, is
//! refused.

use crate::{
    assemble::AssemblyError,
    name_keys::KeyNames,
    stored_json::StoredJson,
    stored_value::{LiteralAnnotation, StoredValue},
};
use andromeda_ngsi_ld_entity::{
    expanded_value::ExpandedValue,
    family_value::FamilyValue,
    geo_value::{EncodedGeometry, GeoValue},
    geometry_value::{ExpandedGeoJsonMembers, GeometryValue},
    list_object::ListObject,
    literal::{Literal, LiteralValue},
    non_empty::NonEmpty,
    value_mode::Fragment,
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    datatype::declared_value::NumberSpelling,
    reference::NgsiLdReference,
    time::{rendering::instant_text, value::TimeValue},
    uri::Uri,
};
use andromeda_ngsi_ld_model::temporal::structured_value::StructuredValue;
use andromeda_postgresql_codes::value_kind::ValueKind;
use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
use cefact_units::UnitCode;

/// The members only some families define, as stored.
pub(crate) struct Envelope {
    /// `unitCode`.
    pub(crate) unit_code: Option<UnitCode>,
    /// `valueType`.
    pub(crate) value_type: Option<NgsiLdReference>,
    /// `objectType`.
    pub(crate) object_types: Vec<NgsiLdReference>,
}

/// One stored instance whose family value is being read.
#[derive(Clone, Copy)]
pub(crate) struct StoredFamily<'a> {
    /// The attribute, named in a refusal.
    pub(crate) name: &'a AttributeName,
    /// The family the attribute kind names.
    pub(crate) family: AttributeType,
    /// The names behind the stored type keys.
    pub(crate) names: &'a KeyNames,
}

impl StoredFamily<'_> {
    /// The family value of a stored instance with `value` and `envelope`.
    pub(crate) fn value(self, value: StoredValue, envelope: Envelope) -> Result<FamilyValue, AssemblyError> {
        let Envelope {
            unit_code,
            value_type,
            object_types,
        } = envelope;
        match self.family {
            AttributeType::Property => {
                self.refuse_present(!object_types.is_empty())?;
                Ok(FamilyValue::Property {
                    values: self.property_values(value, value_type.as_ref())?,
                    unit_code,
                    value_type,
                })
            }
            AttributeType::GeoProperty => {
                self.refuse_present(!object_types.is_empty())?;
                Ok(FamilyValue::GeoProperty {
                    geometry: self.geometry(value)?,
                    unit_code,
                    value_type,
                })
            }
            AttributeType::Relationship => {
                self.refuse_present(unit_code.is_some() || value_type.is_some())?;
                Ok(FamilyValue::Relationship {
                    objects: self.objects(value)?,
                    object_types,
                })
            }
            AttributeType::LanguageProperty => {
                self.refuse_present(unit_code.is_some() || !object_types.is_empty())?;
                Ok(FamilyValue::LanguageProperty {
                    entries: match self.structured(value)? {
                        StructuredValue::LanguageMap(entries) => entries.into_stored().ok_or_else(|| self.null_in_state())?,
                        StructuredValue::Values(_)
                        | StructuredValue::Objects(_)
                        | StructuredValue::Vocabs(_)
                        | StructuredValue::Json(_)
                        | StructuredValue::List(_)
                        | StructuredValue::ObjectList(_) => return Err(self.mismatch(ValueKind::Object)),
                    },
                    value_type,
                })
            }
            AttributeType::VocabProperty => {
                self.refuse_present(unit_code.is_some() || !object_types.is_empty())?;
                Ok(FamilyValue::VocabProperty {
                    vocab: self.vocab(value)?,
                    value_type,
                })
            }
            AttributeType::JsonProperty => {
                self.refuse_present(unit_code.is_some() || !object_types.is_empty())?;
                Ok(FamilyValue::JsonProperty {
                    json: match self.structured(value)? {
                        StructuredValue::Json(literal) => NonEmpty::one(literal),
                        StructuredValue::Values(_)
                        | StructuredValue::Objects(_)
                        | StructuredValue::Vocabs(_)
                        | StructuredValue::LanguageMap(_)
                        | StructuredValue::List(_)
                        | StructuredValue::ObjectList(_) => return Err(self.mismatch(ValueKind::Object)),
                    },
                    value_type,
                })
            }
            AttributeType::ListProperty => {
                self.refuse_present(!object_types.is_empty())?;
                Ok(FamilyValue::ListProperty {
                    items: match self.structured(value)? {
                        StructuredValue::List(items) => self.stored_values(items)?,
                        StructuredValue::Values(_)
                        | StructuredValue::Objects(_)
                        | StructuredValue::Vocabs(_)
                        | StructuredValue::LanguageMap(_)
                        | StructuredValue::Json(_)
                        | StructuredValue::ObjectList(_) => return Err(self.mismatch(ValueKind::Object)),
                    },
                    unit_code,
                    value_type,
                })
            }
            AttributeType::ListRelationship => {
                self.refuse_present(unit_code.is_some() || value_type.is_some())?;
                Ok(FamilyValue::ListRelationship {
                    objects: match self.structured(value)? {
                        StructuredValue::ObjectList(targets) => targets.into_iter().map(ListObject::Wrapped).collect(),
                        StructuredValue::Values(_)
                        | StructuredValue::Objects(_)
                        | StructuredValue::Vocabs(_)
                        | StructuredValue::LanguageMap(_)
                        | StructuredValue::Json(_)
                        | StructuredValue::List(_) => return Err(self.mismatch(ValueKind::Object)),
                    },
                    object_types,
                })
            }
        }
    }

    /// The values of a Property beside the `valueType` `value_type`: one
    /// literal from a scalar column, else the stored structured value.
    fn property_values(self, value: StoredValue, value_type: Option<&NgsiLdReference>) -> Result<Vec<ExpandedValue>, AssemblyError> {
        match value {
            StoredValue::Object(json) => match self.read(&json)? {
                StructuredValue::Values(values) => self.stored_values(values),
                StructuredValue::Objects(_)
                | StructuredValue::Vocabs(_)
                | StructuredValue::LanguageMap(_)
                | StructuredValue::Json(_)
                | StructuredValue::List(_)
                | StructuredValue::ObjectList(_) => Err(self.mismatch(ValueKind::Object)),
            },
            scalar @ (StoredValue::Number { .. }
            | StoredValue::String { .. }
            | StoredValue::Boolean { .. }
            | StoredValue::DeclaredNumber { .. }
            | StoredValue::DeclaredBoolean { .. }
            | StoredValue::DateTime { .. }
            | StoredValue::Date { .. }
            | StoredValue::Time { .. }) => Ok(vec![ExpandedValue::Literal(self.literal(scalar, value_type)?)]),
            other @ (StoredValue::Geometry { .. } | StoredValue::Uri(_)) => Err(self.mismatch(other.kind())),
        }
    }

    /// The geometry of a `GeoProperty`: positions with their members, or the
    /// clause 4.7.2 string.
    fn geometry(self, value: StoredValue) -> Result<GeoValue, AssemblyError> {
        match value {
            StoredValue::Geometry { form, members } => {
                let members = members
                    .map(|members| ExpandedGeoJsonMembers::from_stored(self.name, members.get()))
                    .transpose()?
                    .flatten();
                Ok(GeoValue::Geometry(GeometryValue::with_members(form, members)))
            }
            StoredValue::String {
                text,
                annotation: LiteralAnnotation::Plain,
            } => Ok(GeoValue::Encoded(EncodedGeometry::parse(text)?)),
            other @ (StoredValue::Number { .. }
            | StoredValue::String { .. }
            | StoredValue::Boolean { .. }
            | StoredValue::DeclaredNumber { .. }
            | StoredValue::DeclaredBoolean { .. }
            | StoredValue::DateTime { .. }
            | StoredValue::Date { .. }
            | StoredValue::Time { .. }
            | StoredValue::Object(_)
            | StoredValue::Uri(_)) => Err(self.mismatch(other.kind())),
        }
    }

    /// The objects of a Relationship: one inline, several as a structured
    /// value.
    fn objects(self, value: StoredValue) -> Result<NonEmpty<Uri>, AssemblyError> {
        match value {
            StoredValue::Uri(object) => Ok(NonEmpty::one(object)),
            StoredValue::Object(json) => match self.read(&json)? {
                StructuredValue::Objects(objects) => Ok(objects),
                StructuredValue::Values(_)
                | StructuredValue::Vocabs(_)
                | StructuredValue::LanguageMap(_)
                | StructuredValue::Json(_)
                | StructuredValue::List(_)
                | StructuredValue::ObjectList(_) => Err(self.mismatch(ValueKind::Object)),
            },
            other @ (StoredValue::Number { .. }
            | StoredValue::String { .. }
            | StoredValue::Boolean { .. }
            | StoredValue::DeclaredNumber { .. }
            | StoredValue::DeclaredBoolean { .. }
            | StoredValue::DateTime { .. }
            | StoredValue::Date { .. }
            | StoredValue::Time { .. }
            | StoredValue::Geometry { .. }) => Err(self.mismatch(other.kind())),
        }
    }

    /// The terms of a `VocabProperty`: one as a string, several as a
    /// structured value.
    fn vocab(self, value: StoredValue) -> Result<NonEmpty<NgsiLdReference>, AssemblyError> {
        match value {
            StoredValue::String {
                text,
                annotation: LiteralAnnotation::Plain,
            } => Ok(NonEmpty::one(text.parse()?)),
            StoredValue::Object(json) => match self.read(&json)? {
                StructuredValue::Vocabs(vocab) => Ok(vocab),
                StructuredValue::Values(_)
                | StructuredValue::Objects(_)
                | StructuredValue::LanguageMap(_)
                | StructuredValue::Json(_)
                | StructuredValue::List(_)
                | StructuredValue::ObjectList(_) => Err(self.mismatch(ValueKind::Object)),
            },
            other @ (StoredValue::Number { .. }
            | StoredValue::String { .. }
            | StoredValue::Boolean { .. }
            | StoredValue::DeclaredNumber { .. }
            | StoredValue::DeclaredBoolean { .. }
            | StoredValue::DateTime { .. }
            | StoredValue::Date { .. }
            | StoredValue::Time { .. }
            | StoredValue::Geometry { .. }
            | StoredValue::Uri(_)) => Err(self.mismatch(other.kind())),
        }
    }

    /// The literal a scalar column holds beside the `valueType` `value_type`.
    ///
    /// A declared number or Boolean is the JSON string it was written as, in
    /// the canonical lexical form of the datatype that declares it, the
    /// `valueType` before the literal's own.
    fn literal(self, value: StoredValue, value_type: Option<&NgsiLdReference>) -> Result<Literal, AssemblyError> {
        let datatype = |key: Option<EntityTypeNameKey>| key.map(|key| self.type_name(key)).transpose();
        Ok(match value {
            StoredValue::Number { number, datatype: key } => Literal::try_new(LiteralValue::Number(number), datatype(key)?, None, None)?,
            StoredValue::Boolean { flag, datatype: key } => Literal::try_new(LiteralValue::Boolean(flag), datatype(key)?, None, None)?,
            StoredValue::DeclaredNumber { number, datatype: key } => {
                let datatype = datatype(key)?;
                let spelling = value_type
                    .or(datatype.as_ref())
                    .and_then(NumberSpelling::named_by)
                    .ok_or_else(|| self.mismatch(ValueKind::DeclaredNumber))?;
                Literal::try_new(LiteralValue::String(spelling.spell(&number).into()), datatype, None, None)?
            }
            StoredValue::DeclaredBoolean { flag, datatype: key } => {
                Literal::try_new(LiteralValue::String(flag.to_string().into()), datatype(key)?, None, None)?
            }
            StoredValue::DateTime { instant, datatype: key } => {
                Literal::try_new(LiteralValue::String(instant_text(instant).into()), datatype(key)?, None, None)?
            }
            StoredValue::Date { date, datatype: key } => {
                Literal::try_new(LiteralValue::String(TimeValue::Date(date).to_string().into()), datatype(key)?, None, None)?
            }
            StoredValue::Time { time, datatype: key } => {
                Literal::try_new(LiteralValue::String(TimeValue::Time(time).to_string().into()), datatype(key)?, None, None)?
            }
            StoredValue::String { text, annotation } => match annotation {
                LiteralAnnotation::Plain => Literal::plain(LiteralValue::String(text.into())),
                LiteralAnnotation::Datatype(key) => Literal::typed(LiteralValue::String(text.into()), self.type_name(key)?),
                LiteralAnnotation::Language(language) => Literal::try_new(LiteralValue::String(text.into()), None, Some(language), None)?,
            },
            other @ (StoredValue::Geometry { .. } | StoredValue::Object(_) | StoredValue::Uri(_)) => return Err(self.mismatch(other.kind())),
        })
    }

    /// The structured value of a family held only in `value_object`.
    fn structured(self, value: StoredValue) -> Result<StructuredValue, AssemblyError> {
        match value {
            StoredValue::Object(json) => self.read(&json),
            other @ (StoredValue::Number { .. }
            | StoredValue::String { .. }
            | StoredValue::Boolean { .. }
            | StoredValue::DeclaredNumber { .. }
            | StoredValue::DeclaredBoolean { .. }
            | StoredValue::DateTime { .. }
            | StoredValue::Date { .. }
            | StoredValue::Time { .. }
            | StoredValue::Geometry { .. }
            | StoredValue::Uri(_)) => Err(self.mismatch(other.kind())),
        }
    }

    /// Read stored JSON as the structured value of the family.
    fn read(self, json: &StoredJson) -> Result<StructuredValue, AssemblyError> {
        Ok(StructuredValue::from_stored(self.family, self.name, json.get())?)
    }

    /// Values read from a stored structured value, which hold no deletion.
    fn stored_values(self, values: Vec<ExpandedValue<Fragment>>) -> Result<Vec<ExpandedValue>, AssemblyError> {
        values
            .into_iter()
            .map(|value| value.into_stored().ok_or_else(|| self.null_in_state()))
            .collect()
    }

    /// The type IRI behind a type key.
    pub(crate) fn type_name(self, key: EntityTypeNameKey) -> Result<NgsiLdReference, AssemblyError> {
        // clone: the assembled state owns its type IRIs
        self.names.type_name(key).cloned().ok_or(AssemblyError::UnknownType(key))
    }

    /// Refuse a member the family does not define when it is present.
    fn refuse_present(self, present: bool) -> Result<(), AssemblyError> {
        if present {
            return Err(AssemblyError::ForeignMember {
                attribute: self.name.clone(),
                family: self.family,
            });
        }
        Ok(())
    }

    /// A value of `kind` the family does not hold.
    fn mismatch(self, kind: ValueKind) -> AssemblyError {
        AssemblyError::ValueOfAnotherFamily {
            attribute: self.name.clone(),
            family: self.family,
            kind,
        }
    }

    /// A stored structured value holding the NGSI-LD Null.
    fn null_in_state(self) -> AssemblyError {
        AssemblyError::NullInState(self.name.clone())
    }
}

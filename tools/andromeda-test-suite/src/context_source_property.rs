//! The Property form of a Context Source Property: a registration member that a context source filter targets.

use indexmap::IndexMap;
use ngsi_ld::model::Value;

/// The member value `{"type": "Property", "value": value}` of a Context Source Property (clause 4.9).
#[must_use]
pub fn property_value(value: &str) -> Value {
    Value::Object(IndexMap::from([
        ("type".to_owned(), Value::from("Property")),
        ("value".to_owned(), Value::from(value)),
    ]))
}

/// The member value `{"type": "LanguageProperty", "languageMap": {..}}` of a Context Source Property holding one string
/// per language tag in `entries` (clause 4.15).
#[must_use]
pub fn language_property_value(entries: &[(&str, &str)]) -> Value {
    let language_map = entries.iter().map(|(tag, text)| ((*tag).to_owned(), Value::from(*text))).collect();
    Value::Object(IndexMap::from([
        ("type".to_owned(), Value::from("LanguageProperty")),
        ("languageMap".to_owned(), Value::Object(language_map)),
    ]))
}

#[cfg(test)]
mod tests {
    use crate::{
        context_source_property::{language_property_value, property_value},
        namespace::Namespace,
    };
    use googletest::prelude::*;
    use ngsi_ld::{
        model::Value,
        registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
    };

    fn rendered(name: &str, value: Value) -> Result<String> {
        let namespace = Namespace::fresh();
        let entity_type = namespace.entity_type("Building")?;
        let registration = CSourceRegistration::builder("http://source.test/")
            .id(namespace.registration_id(1)?)
            .information([RegistrationInfo::builder().entities([EntityInfo::new(entity_type)]).build()?])
            .property(name, value)
            .build()?;
        Ok(registration.render().to_string())
    }

    #[gtest]
    fn a_registration_writes_the_property_with_its_wrapper() -> Result<()> {
        verify_that!(
            rendered("tier", property_value("gold"))?,
            contains_substring(r#""tier":{"type":"Property","value":"gold"}"#)
        )
    }

    #[gtest]
    fn a_registration_writes_the_language_property_with_its_language_map() -> Result<()> {
        verify_that!(
            rendered("fleetLabel", language_property_value(&[("en", "Fleet"), ("fr", "Flotte")]))?,
            contains_substring(r#""fleetLabel":{"type":"LanguageProperty","languageMap":{"en":"Fleet","fr":"Flotte"}}"#)
        )
    }
}

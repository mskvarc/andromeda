//! A matcher on the language map of a `LanguageProperty` an entity holds: exactly the languages and texts the test
//! names, so a language the update removed is checked absent (clause 4.5.18).

use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use ngsi_ld::model::{Attribute, Entity, Output};

/// Matches an entity whose `LanguageProperty` `name` holds exactly the language map `pairs`.
#[derive(Debug, Clone, MatcherBase)]
pub struct HasLanguageMap {
    /// The attribute name, as the answer's terms write it.
    name: String,
    /// The language tag and text of each entry, in any order.
    pairs: Vec<(String, String)>,
}

/// An entity whose `LanguageProperty` `name` holds exactly the entries `pairs` (language tag, text), no other.
#[must_use]
pub fn has_language_map(name: &str, pairs: &[(&str, &str)]) -> HasLanguageMap {
    HasLanguageMap {
        name: name.to_owned(),
        pairs: pairs.iter().map(|(language, text)| ((*language).to_owned(), (*text).to_owned())).collect(),
    }
}

impl HasLanguageMap {
    /// Whether `attribute` is a `LanguageProperty` holding exactly the entries.
    fn holds(&self, attribute: &Attribute<Output>) -> bool {
        match attribute {
            Attribute::LanguageProperty(property) => {
                let map = property.language_map();
                map.iter().count() == self.pairs.len()
                    && self
                        .pairs
                        .iter()
                        .all(|(language, text)| map.iter().any(|(key, value)| key.to_string() == *language && *value == text.as_str().into()))
            }
            Attribute::Property(_)
            | Attribute::GeoProperty(_)
            | Attribute::VocabProperty(_)
            | Attribute::ListProperty(_)
            | Attribute::JsonProperty(_)
            | Attribute::Relationship(_)
            | Attribute::ListRelationship(_) => false,
        }
    }
}

impl Matcher<&Entity<Output>> for HasLanguageMap {
    fn matches(&self, actual: &Entity<Output>) -> MatcherResult {
        actual
            .attributes
            .iter()
            .filter(|(candidate, _)| candidate.as_str() == self.name)
            .any(|(_, instances)| instances.iter().any(|instance| self.holds(instance)))
            .into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("has a language property {} holding exactly {:?}", self.name, self.pairs).into(),
            MatcherResult::NoMatch => format!("hasn't a language property {} holding exactly {:?}", self.name, self.pairs).into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::language_map_matcher::has_language_map;
    use googletest::prelude::*;
    use ngsi_ld::model::{Entity, Lossless, Output};

    /// A building whose street names two languages.
    fn building() -> Result<Entity<Output>> {
        Ok(Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:Building:1","type":"Building","street":{"type":"LanguageProperty","languageMap":{"en":"Grand Place","es":"Gran Lugar"}}}"#,
            Lossless::Normalized,
        )?)
    }

    #[gtest]
    fn the_entries_are_matched_in_any_order() -> Result<()> {
        let entity = building()?;
        verify_that!(&entity, has_language_map("street", &[("es", "Gran Lugar"), ("en", "Grand Place")]))
    }

    #[gtest]
    fn a_missing_or_an_extra_entry_does_not_match() -> Result<()> {
        let entity = building()?;
        expect_that!(&entity, not(has_language_map("street", &[("en", "Grand Place")])));
        expect_that!(
            &entity,
            not(has_language_map(
                "street",
                &[("en", "Grand Place"), ("es", "Gran Lugar"), ("fr", "Grand-Place")]
            ))
        );
        verify_that!(&entity, not(has_language_map("street", &[("en", "Grand Place"), ("es", "Otro")])))
    }
}

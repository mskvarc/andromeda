//! Text of entities and attribute fragments whose one member holds millions of entries: the members the entity value
//! limit counts by the entries they hold (`object`, `objectList`, `vocab`, `languageMap`, `scope`).

use strum::EnumIter;

/// The member of an entity that holds the entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq, EnumIter)]
pub enum ManyEntries {
    /// The `value` of a `Property`, one number per entry.
    Numbers,
    /// The `object` of a `Relationship`, one URN per entry.
    Targets,
    /// The `objectList` of a `ListRelationship`, one URN per entry.
    ObjectList,
    /// The `vocab` of a `VocabProperty`, one term per entry.
    Terms,
    /// The `languageMap` of a `LanguageProperty` holding one tag with an array of strings.
    Strings,
    /// The `languageMap` of a `LanguageProperty` holding one string per tag.
    Tags,
    /// The `scope` of the entity, one path per entry.
    Scopes,
}

impl ManyEntries {
    /// The text of the attribute that holds `count` entries, `None` for the entity `scope`, which is no attribute.
    #[must_use]
    pub fn attribute(self, count: usize) -> Option<String> {
        match self {
            ManyEntries::Numbers => Some(format!(r#"{{"type":"Property","value":[{}]}}"#, entries(count, |number| number.to_string()))),
            ManyEntries::Targets => Some(format!(r#"{{"type":"Relationship","object":[{}]}}"#, urns(count))),
            ManyEntries::ObjectList => Some(format!(r#"{{"type":"ListRelationship","objectList":[{}]}}"#, urns(count))),
            ManyEntries::Terms => Some(format!(
                r#"{{"type":"VocabProperty","vocab":[{}]}}"#,
                entries(count, |number| format!(r#""term{number}""#))
            )),
            ManyEntries::Strings => Some(format!(
                r#"{{"type":"LanguageProperty","languageMap":{{"en":[{}]}}}}"#,
                entries(count, |_| r#""s""#.to_owned())
            )),
            ManyEntries::Tags => Some(format!(
                r#"{{"type":"LanguageProperty","languageMap":{{{}}}}}"#,
                entries(count, |number| format!(r#""x-{number}":"s""#))
            )),
            ManyEntries::Scopes => None,
        }
    }

    /// The text of the members of an entity fragment (the body of a merge, an update or an append) that hold `count`
    /// entries, the attribute named `member` for the attributes.
    #[must_use]
    pub fn fragment(self, member: &str, count: usize) -> String {
        match self.attribute(count) {
            Some(attribute) => format!(r#"{{"{member}":{attribute}}}"#),
            None => format!(r#"{{"scope":[{}]}}"#, scopes(count)),
        }
    }

    /// The text of an entity of `id` and `entity_type` whose one member holds `count` entries, the attribute named
    /// `member` for the attributes.
    #[must_use]
    pub fn entity(self, id: &str, entity_type: &str, member: &str, count: usize) -> String {
        let fragment = self.fragment(member, count);
        format!(r#"{{"id":"{id}","type":"{entity_type}",{}"#, fragment.trim_start_matches('{'))
    }

    /// [`ManyEntries::entity`] under a body `@context` that makes the typed expansion decline the attribute `member`.
    #[must_use]
    pub fn declined_entity(self, id: &str, entity_type: &str, member: &str, count: usize) -> String {
        declined(&self.entity(id, entity_type, member, count), member)
    }

    /// [`ManyEntries::fragment`] under a body `@context` that makes the typed expansion decline the attribute `member`.
    #[must_use]
    pub fn declined_fragment(self, member: &str, count: usize) -> String {
        declined(&self.fragment(member, count), member)
    }
}

/// The body `@context` that gives the attribute `member` a property-scoped context which does not propagate, which the
/// typed expansion declines wherever the attribute is written, so the JSON-LD processor expands the request.
fn declining_context(member: &str) -> String {
    format!(r#"[{{"{member}":{{"@id":"https://example.org/declined/{member}","@context":{{"@propagate":false}}}}}}]"#)
}

/// `text`, a JSON object, with the body `@context` that makes the typed expansion decline the attribute `member`.
fn declined(text: &str, member: &str) -> String {
    format!(r#"{{"@context":{},{}"#, declining_context(member), text.trim_start_matches('{'))
}

/// `count` URNs written as the entries of a JSON array.
fn urns(count: usize) -> String {
    entries(count, |number| format!(r#""urn:ngsi-ld:Target:{number}""#))
}

/// `count` distinct scope paths written as the entries of a JSON array.
fn scopes(count: usize) -> String {
    entries(count, |number| format!(r#""/s{number}""#))
}

/// The entries `entry` writes for `0..count`, separated by commas.
fn entries(count: usize, entry: impl Fn(usize) -> String) -> String {
    (0..count).map(entry).collect::<Vec<_>>().join(",")
}

#[cfg(test)]
mod tests {
    use crate::many_entries::ManyEntries;
    use googletest::prelude::*;
    use serde_json::Value;
    use strum::IntoEnumIterator;

    #[gtest]
    fn every_member_is_written_as_json() -> Result<()> {
        for member in ManyEntries::iter() {
            let entity = member.entity("urn:ngsi-ld:Probe:1", "Probe", "m", 3);
            let parsed: Value = serde_json::from_str(&entity)?;
            expect_that!(parsed.get("id").and_then(Value::as_str), some(eq("urn:ngsi-ld:Probe:1")));
            expect_that!(parsed.get("type").and_then(Value::as_str), some(eq("Probe")));
            let fragment: Value = serde_json::from_str(&member.fragment("m", 3))?;
            expect_that!(fragment.as_object().map(serde_json::Map::len), some(eq(1)));
        }
        Ok(())
    }

    #[gtest]
    fn a_declined_entity_names_the_context_that_makes_the_expansion_decline_its_attribute() -> Result<()> {
        let entity: Value = serde_json::from_str(&ManyEntries::Numbers.declined_entity("urn:ngsi-ld:Probe:1", "Probe", "m", 3))?;
        verify_that!(entity.pointer("/@context/0/m/@context/@propagate").and_then(Value::as_bool), some(eq(false)))?;
        verify_that!(entity.pointer("/m/value").and_then(Value::as_array).map(Vec::len), some(eq(3)))?;
        let fragment: Value = serde_json::from_str(&ManyEntries::Targets.declined_fragment("m", 2))?;
        verify_that!(fragment.pointer("/m/object").and_then(Value::as_array).map(Vec::len), some(eq(2)))
    }

    #[gtest]
    fn a_relationship_holds_the_asked_number_of_targets() -> Result<()> {
        let attribute: Value = serde_json::from_str(&ManyEntries::Targets.attribute(4).or_fail()?)?;
        verify_that!(attribute.get("object").and_then(Value::as_array).map(Vec::len), some(eq(4)))
    }

    #[gtest]
    fn a_language_map_of_tags_holds_one_string_per_tag() -> Result<()> {
        let attribute: Value = serde_json::from_str(&ManyEntries::Tags.attribute(5).or_fail()?)?;
        verify_that!(attribute.get("languageMap").and_then(Value::as_object).map(serde_json::Map::len), some(eq(5)))
    }

    #[gtest]
    fn the_scope_is_no_attribute_and_holds_distinct_paths() -> Result<()> {
        expect_that!(ManyEntries::Scopes.attribute(2), none());
        let fragment: Value = serde_json::from_str(&ManyEntries::Scopes.fragment("m", 3))?;
        verify_that!(
            fragment
                .get("scope")
                .and_then(Value::as_array)
                .map(|scopes| scopes.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
            some(elements_are![eq(&"/s0"), eq(&"/s1"), eq(&"/s2")])
        )
    }
}

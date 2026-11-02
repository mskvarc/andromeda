//! The compact `scope` member of an Entity (ETSI GS CIM 009 clause 4.18).
//!
//! Clause 4.18 writes one Scope as a string and several as a JSON array of
//! strings, and reserves the NGSI-LD Null for a deleted Scope. The member is
//! read item by item through [`scope_slot`], the reading the expanded document,
//! the stored entity and a reified temporal Scope already go through, so a
//! request decoded into this model meets exactly the grammar, bounds, Null rule
//! and de-duplication every other reader applies.

use crate::attribute::one_or_many::OneOrMany;
use andromeda_ngsi_ld_entity::{
    document::slot::Slot,
    scope_refusal::ScopeRefusal,
    scope_slot::{ScopeItem, scope_slot},
    scopes::Scopes,
};
use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
use andromeda_ngsi_ld_vocabulary::ngsi_ld_null::NULL_URI;
use derive_more::From;
use serde::{Deserialize, Serialize, Serializer};

/// The `scope` member as a compact request or response writes it: the Scopes
/// it states, or their deletion.
#[derive(Clone, Debug, Deserialize, Eq, From, PartialEq)]
#[serde(try_from = "OneOrMany<String>")]
pub struct ScopeMember(Slot<Scopes>);

impl ScopeMember {
    /// The Scopes the member states, or the deletion.
    #[must_use]
    pub const fn slot(&self) -> &Slot<Scopes> {
        &self.0
    }
}

// Clause 4.18 writes one Scope as a string and several as an array of
// strings; each item is then read as every other reader reads it.
impl TryFrom<OneOrMany<String>> for ScopeMember {
    type Error = ScopeRefusal;

    fn try_from(written: OneOrMany<String>) -> Result<ScopeMember, ScopeRefusal> {
        match written {
            OneOrMany::One(item) => scope_slot([ScopeItem::Text(&item)]),
            OneOrMany::Many(items) => scope_slot(items.iter().map(|item| ScopeItem::Text(item))),
        }
        .map(ScopeMember)
    }
}

// Hand-written because the shape depends on the value: one Scope is written
// bare and several as an array (clause 4.18), and the deletion is the NGSI-LD
// Null; no derive chooses a representation by element count.
impl Serialize for ScopeMember {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.0 {
            Slot::Set(scopes) => match scopes.as_slice() {
                [only] => serializer.serialize_str(only.as_str()),
                several => serializer.collect_seq(several.iter().map(ScopePath::as_str)),
            },
            Slot::Remove => serializer.serialize_str(NULL_URI.as_str()),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::scope_member::ScopeMember;
    use andromeda_ngsi_ld_entity::{document::slot::Slot, scopes::Scopes};
    use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
    use serde_json::{Value, json};

    fn member(value: &Value) -> ScopeMember {
        serde_json::from_value(value.clone()).expect("a scope member decodes") // clone: from_value consumes its input
    }

    fn scopes(paths: &[&str]) -> Scopes {
        Scopes::try_from_paths(paths.iter().map(|path| path.parse::<ScopePath>().expect("a valid scope path"))).expect("at least one path")
    }

    /// Clause 4.18 EXAMPLE 2 writes a Scope without its leading `/`; it reads
    /// and writes back as written.
    #[test]
    fn one_scope_reads_and_writes_bare() {
        let read = member(&json!("Madrid"));
        pretty_assertions::assert_eq!(read.slot(), &Slot::Set(scopes(&["Madrid"])));
        pretty_assertions::assert_eq!(serde_json::to_value(&read).expect("serializes"), json!("Madrid"));
    }

    /// Clause 4.18: "The value of scope is represented as a JSON array in case
    /// there is more than one Scope"; a repeated spelling is kept once.
    #[test]
    fn several_scopes_read_and_write_as_an_array_in_order() {
        let read = member(&json!(["/A", "B", "/A"]));
        pretty_assertions::assert_eq!(read.slot(), &Slot::Set(scopes(&["/A", "B"])));
        pretty_assertions::assert_eq!(serde_json::to_value(&read).expect("serializes"), json!(["/A", "B"]));
    }

    /// Clause 4.18: the NGSI-LD Null "shall be only used and only appear in
    /// case of deleted scopes".
    #[test]
    fn the_null_alone_reads_as_the_deletion() {
        let read = member(&json!("urn:ngsi-ld:null"));
        pretty_assertions::assert_eq!(read.slot(), &Slot::Remove);
        pretty_assertions::assert_eq!(serde_json::to_value(&read).expect("serializes"), json!("urn:ngsi-ld:null"));
    }

    #[test]
    fn a_value_outside_the_grammar_is_refused() {
        for value in [json!("/A B"), json!([]), json!(["/A", "urn:ngsi-ld:null"]), json!([1])] {
            assert!(serde_json::from_value::<ScopeMember>(value.clone()).is_err(), "{value}"); // clone: from_value consumes its input
        }
    }
}

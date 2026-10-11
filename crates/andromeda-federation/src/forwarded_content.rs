//! What a forwarded write carries: the content the clause 5.5.4 admission
//! expanded, read by the forwarding phase (clauses 5.6.1.4 and following).
//!
//! A write handler hands the forwarding layer the admitted document itself,
//! never a serialisation of what the client sent. Registration matching
//! (clause 5.12) needs only the entity identifier and type, so a write no
//! registration covers -- the common case -- never compacts its content at
//! all. The names of the content are the expanded names the admission
//! produced, so the coverage of a registration is decided on what a name means
//! and the spelling a source receives is decided by compaction under that
//! registration's forwarding context (clause 4.3.6.6).

use andromeda_ngsi_ld_entity::document::{EntityDocument, instance_document::InstanceDocument};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// The admitted content of one forwarded write.
#[derive(Debug, Clone, Copy)]
pub enum ForwardedContent<'a> {
    /// An entity or an entity-shaped fragment: the payload of clauses 5.6.1 to
    /// 5.6.3, 5.6.17 and 5.6.18, or the Temporal Evolution of clauses 5.6.11,
    /// 5.6.12 and 5.6.14.
    Entity(&'a EntityDocument),
    /// A Temporal Evolution whose Scope is reified (clauses 4.5.6 and
    /// 5.6.11.4): the instances it adds, and the Scope history it dates.
    Evolution {
        /// The instances the evolution adds, which registrations are matched
        /// and narrowed on.
        document: &'a EntityDocument,
        /// The reified Scope instances, held as the one member keyed by the
        /// Scope IRI. Like the Scope paths of an entity, it is no attribute a
        /// registration declares: it goes wherever the evolution does.
        scope_history: &'a EntityDocument,
    },
    /// The one attribute instance a request path names (clauses 5.6.4 and
    /// 5.6.19).
    Attribute {
        /// The attribute's expanded name.
        name: &'a AttributeName,
        /// The admitted instance.
        instance: &'a InstanceDocument,
        /// What the instance writes of the attribute.
        write: AttributeWrite,
    },
}

/// What the one attribute instance a request path names writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeWrite {
    /// Clause 5.6.4: a fragment merged into the attribute held (clause
    /// 5.5.12), which may leave out its `type` and its value member.
    Fragment,
    /// Clause 5.6.19: the whole attribute, in place of the one held.
    Replacement,
}

impl ForwardedContent<'_> {
    /// The expanded names of the attributes the content writes.
    pub(crate) fn written_attributes(&self) -> Vec<AttributeName> {
        match self {
            ForwardedContent::Entity(document) | ForwardedContent::Evolution { document, .. } => document.attributes().keys().cloned().collect(), // clone: the coverage check owns the names it reports
            ForwardedContent::Attribute { name, .. } => vec![(*name).clone()],                                                                    // clone: same
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::forwarded_content::{AttributeWrite, ForwardedContent};
    use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use indexmap::IndexMap;

    fn name(raw: &str) -> AttributeName {
        raw.parse().expect("a valid attribute name")
    }

    /// An entity document writes the attributes it holds, in written order.
    #[test]
    fn an_entity_writes_the_attributes_its_document_holds() {
        let document = EntityDocument::new(
            None,
            Vec::new(),
            None,
            None,
            IndexMap::from([
                (
                    name("https://example.org/speed"),
                    AttributeMember::Instances(vec![InstanceDocument::builder().build()]),
                ),
                (name("https://example.org/brand"), AttributeMember::Remove),
            ]),
        );
        pretty_assertions::assert_eq!(
            ForwardedContent::Entity(&document).written_attributes(),
            vec![name("https://example.org/speed"), name("https://example.org/brand")]
        );
    }

    /// An attribute fragment writes the one attribute its path names.
    #[test]
    fn an_attribute_fragment_writes_its_own_attribute() {
        let attribute = name("https://example.org/speed");
        let instance = InstanceDocument::builder().build();
        pretty_assertions::assert_eq!(
            ForwardedContent::Attribute {
                name: &attribute,
                instance: &instance,
                write: AttributeWrite::Fragment,
            }
            .written_attributes(),
            vec![attribute.clone()]
        );
    }
}

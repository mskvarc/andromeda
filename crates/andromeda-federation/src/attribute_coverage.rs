//! A registration stores the attribute names it covers expanded, and the admitted content of a write names its attributes expanded too, so deciding whether a registration covers an attribute compares two IRIs: what the name means decides, even under a `@context` that aliases a term to an unrelated IRI. A write body may carry no `@context` at all (clause 6.3.5 makes `application/json` without a context `Link` legal, resolving against the core context) while the registration's declared names were expanded under a different one that the registration does not retain. The two then cannot meet on an IRI and the registration does not cover the attribute: clause 5.12 matches expanded names only, so two different IRIs never match, whatever they share at their tail.

use crate::registration_information::information_matches_entity;
use andromeda_ngsi_ld_entity::document::EntityDocument;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use iri_rs::IriBuf;
use std::borrow::Cow;

/// The attributes a registration covers for one entity, read off the
/// information entries that match it (clause 5.12).
enum AttributeCoverage {
    /// No information entry matches the entity: nothing is covered.
    NotMatching,
    /// A matching entry declares no names, so every attribute is covered.
    Unrestricted,
    /// Only these declared names, stored expanded.
    Names(Vec<AttributeName>),
}

/// What `csr` covers of an entity's attributes.
fn attribute_coverage(csr: &ContextSourceRegistration, entity_types: &[IriBuf], entity_id: Option<&Identifier>) -> AttributeCoverage {
    let information = csr
        .coverage
        .information
        .iter()
        .filter(|information| information_matches_entity(information, entity_types, entity_id))
        .collect::<Vec<_>>();
    if information.is_empty() {
        return AttributeCoverage::NotMatching;
    }
    let unrestricted = information
        .iter()
        .any(|entry| entry.property_names.as_ref().is_none_or(Vec::is_empty) && entry.relationship_names.as_ref().is_none_or(Vec::is_empty));
    if unrestricted {
        return AttributeCoverage::Unrestricted;
    }
    AttributeCoverage::Names(
        information
            .iter()
            .flat_map(|entry| entry.property_names.iter().chain(&entry.relationship_names).flatten())
            .cloned() // clone: the coverage owns the declared names borrowed from the registration
            .collect(),
    )
}

/// The attributes among `requested` that a registration declaring `declared`
/// covers.
fn covered_among<'a>(requested: impl Iterator<Item = &'a AttributeName>, declared: &[AttributeName]) -> Vec<AttributeName> {
    requested
        .filter(|attribute| declared.contains(attribute))
        .cloned() // clone: the result outlives the borrow of the content
        .collect()
}

/// The attributes of a write that `csr` covers: the one `explicit_attribute` a
/// request path names, else those of `requested` the matching information
/// entries declare, or every requested attribute when an entry declares none,
/// or every declared attribute when the write names none.
pub(crate) fn matching_attributes(
    csr: &ContextSourceRegistration,
    entity_types: &[IriBuf],
    entity_id: Option<&Identifier>,
    explicit_attribute: Option<&AttributeName>,
    requested: &[AttributeName],
) -> Vec<AttributeName> {
    if let Some(attribute) = explicit_attribute {
        return vec![attribute.clone()]; // clone: the result outlives the borrowed request
    }
    let mut covered = Vec::new();
    for information in csr
        .coverage
        .information
        .iter()
        .filter(|information| information_matches_entity(information, entity_types, entity_id))
    {
        let declared = information
            .property_names
            .iter()
            .chain(&information.relationship_names)
            .flatten()
            .cloned() // clone: the comparison owns the names borrowed from the registration
            .collect::<Vec<_>>();
        if declared.is_empty() {
            covered.extend(requested.iter().cloned()); // clone: the result outlives the borrowed request
        } else if requested.is_empty() {
            covered.extend(declared);
        } else {
            covered.extend(covered_among(requested.iter(), &declared));
        }
    }
    covered.sort();
    covered.dedup();
    covered
}

/// The attributes of `document` that `csr` covers for the entity: what another
/// registration's copy loses when `csr` owns them exclusively (clauses 5.6.7.4
/// and following).
pub(crate) fn covered_attributes(
    document: &EntityDocument,
    csr: &ContextSourceRegistration,
    entity_types: &[IriBuf],
    entity_id: Option<&Identifier>,
) -> Vec<AttributeName> {
    match attribute_coverage(csr, entity_types, entity_id) {
        AttributeCoverage::NotMatching => Vec::new(),
        AttributeCoverage::Unrestricted => document.attributes().keys().cloned().collect(), // clone: the result owns the names it reports
        AttributeCoverage::Names(declared) => covered_among(document.attributes().keys(), &declared),
    }
}

/// `document` narrowed to the attributes `csr` covers for it, or `None` when
/// none remains (clauses 5.6.7.4 and following: "remove non-matching
/// Attributes from the remaining Entities"). A document that loses nothing is
/// borrowed as it is.
pub(crate) fn narrowed_document<'d>(
    document: &'d EntityDocument,
    csr: &ContextSourceRegistration,
    entity_types: &[IriBuf],
    entity_id: Option<&Identifier>,
) -> Option<Cow<'d, EntityDocument>> {
    let covered = covered_attributes(document, csr, entity_types, entity_id);
    if covered.is_empty() {
        return None;
    }
    if covered.len() == document.attributes().len() {
        return Some(Cow::Borrowed(document));
    }
    let mut narrowed = document.clone(); // clone: each registration narrows its own copy
    narrowed.attributes_mut().retain(|name, _| covered.contains(name));
    Some(Cow::Owned(narrowed))
}

#[cfg(test)]
mod tests {
    use crate::attribute_coverage::{covered_attributes, matching_attributes, narrowed_document};
    use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use indexmap::IndexMap;
    use iri_rs::IriBuf;
    use serde_json::{Value, json};
    use std::borrow::Cow;

    fn registration(information: &Value) -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:csr:1",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": [information],
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid registration")
    }

    fn name(raw: &str) -> AttributeName {
        raw.parse().expect("a valid attribute name")
    }

    /// A Vehicle document holding the attributes `names`, each with one empty
    /// instance.
    fn document(names: &[&str]) -> EntityDocument {
        EntityDocument::new(
            None,
            Vec::new(),
            None,
            None,
            names
                .iter()
                .map(|raw| (name(raw), AttributeMember::Instances(vec![InstanceDocument::builder().build()])))
                .collect::<IndexMap<_, _>>(),
        )
    }

    fn vehicle_type() -> IriBuf {
        IriBuf::new("https://example.org/Vehicle".to_string()).expect("valid IRI")
    }

    fn vehicle_id() -> Identifier {
        "urn:ngsi-ld:Vehicle:1".parse().expect("valid URI")
    }

    fn kept(document: &EntityDocument) -> Vec<String> {
        let mut kept = document.attributes().keys().map(ToString::to_string).collect::<Vec<_>>();
        kept.sort();
        kept
    }

    /// A registration naming attributes keeps a member when its expanded name
    /// is one of the declared IRIs, whatever term the client wrote it as.
    #[test]
    fn a_registration_naming_attributes_keeps_them_by_iri() {
        let csr = registration(&json!({
            "entities": [{"type": "https://example.org/Vehicle"}],
            "propertyNames": ["https://example.org/vocab/speed", "https://example.org/brandName"]
        }));
        let written = document(&[
            "https://example.org/vocab/speed",
            "https://example.org/brandName",
            "https://example.org/other/colour",
        ]);
        let narrowed = narrowed_document(&written, &csr, &[vehicle_type()], Some(&vehicle_id())).expect("coverage remains");
        pretty_assertions::assert_eq!(kept(&narrowed), vec!["https://example.org/brandName", "https://example.org/vocab/speed"]);
        assert!(matches!(narrowed, Cow::Owned(_)), "a narrowed document is its own copy");
    }

    #[test]
    fn a_registration_declaring_no_attribute_names_covers_every_attribute() {
        let csr = registration(&json!({"entities": [{"type": "https://example.org/Vehicle"}]}));
        let written = document(&["https://example.org/speed", "https://example.org/brandName"]);
        let narrowed = narrowed_document(&written, &csr, &[vehicle_type()], Some(&vehicle_id())).expect("coverage remains");
        assert!(matches!(narrowed, Cow::Borrowed(_)), "a document that loses nothing is not copied");
        pretty_assertions::assert_eq!(covered_attributes(&written, &csr, &[vehicle_type()], Some(&vehicle_id())).len(), 2);
    }

    #[test]
    fn a_registration_not_matching_the_entity_keeps_nothing() {
        let csr = registration(&json!({"entities": [{"type": "https://example.org/Device"}]}));
        let written = document(&["https://example.org/speed"]);
        assert!(narrowed_document(&written, &csr, &[vehicle_type()], Some(&vehicle_id())).is_none());
        pretty_assertions::assert_eq!(
            covered_attributes(&written, &csr, &[vehicle_type()], Some(&vehicle_id())),
            Vec::<AttributeName>::new()
        );
    }

    #[test]
    fn a_registration_naming_only_absent_attributes_leaves_no_attribute() {
        let csr = registration(&json!({
            "entities": [{"type": "https://example.org/Vehicle"}],
            "propertyNames": ["https://example.org/colour"]
        }));
        let written = document(&["https://example.org/speed"]);
        assert!(narrowed_document(&written, &csr, &[vehicle_type()], Some(&vehicle_id())).is_none());
    }

    /// Clause 5.12 compares expanded names: two different IRIs ending in the
    /// same word are different attributes.
    #[test]
    fn different_iris_sharing_a_last_segment_do_not_match() {
        let csr = registration(&json!({
            "entities": [{"type": "https://example.org/Vehicle"}],
            "propertyNames": ["http://b.example/ns/speed"]
        }));
        let written = document(&["http://a.example/v#speed", "https://uri.etsi.org/ngsi-ld/default-context/speed"]);
        assert!(narrowed_document(&written, &csr, &[vehicle_type()], Some(&vehicle_id())).is_none());
        pretty_assertions::assert_eq!(
            matching_attributes(&csr, &[vehicle_type()], Some(&vehicle_id()), None, &[name("http://a.example/v#speed")]),
            Vec::<AttributeName>::new()
        );
    }

    #[test]
    fn identical_iris_match() {
        let csr = registration(&json!({
            "entities": [{"type": "https://example.org/Vehicle"}],
            "propertyNames": ["http://b.example/ns/speed"]
        }));
        let written = document(&["http://b.example/ns/speed"]);
        let narrowed = narrowed_document(&written, &csr, &[vehicle_type()], Some(&vehicle_id())).expect("coverage remains");
        pretty_assertions::assert_eq!(kept(&narrowed), vec!["http://b.example/ns/speed"]);
    }

    /// An attribute another registration's vocabulary calls by a different
    /// short name is not covered, whatever the term it was written as.
    #[test]
    fn an_unrelated_iri_is_not_covered() {
        let csr = registration(&json!({
            "entities": [{"type": "https://example.org/Vehicle"}],
            "propertyNames": ["https://example.org/speed"]
        }));
        let written = document(&["https://other.example.org/rate"]);
        assert!(narrowed_document(&written, &csr, &[vehicle_type()], Some(&vehicle_id())).is_none());
    }

    /// The attributes a proxied registration claims: the explicit attribute of
    /// a request path, the declared ones when the write names none, and the
    /// intersection otherwise.
    #[test]
    fn matching_attributes_follow_the_declared_names() {
        let csr = registration(&json!({
            "entities": [{"type": "https://example.org/Vehicle"}],
            "propertyNames": ["https://example.org/speed", "https://example.org/brandName"]
        }));
        let types = [vehicle_type()];
        let id = vehicle_id();
        let explicit = name("https://example.org/colour");
        pretty_assertions::assert_eq!(matching_attributes(&csr, &types, Some(&id), Some(&explicit), &[]), vec![explicit.clone()]);
        pretty_assertions::assert_eq!(
            matching_attributes(&csr, &types, Some(&id), None, &[]),
            vec![name("https://example.org/brandName"), name("https://example.org/speed")]
        );
        pretty_assertions::assert_eq!(
            matching_attributes(
                &csr,
                &types,
                Some(&id),
                None,
                &[name("https://example.org/speed"), name("https://example.org/colour")]
            ),
            vec![name("https://example.org/speed")]
        );
    }
}

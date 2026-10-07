//! The routing projection derived from a registration's `information`.
//!
//! A registration's `information` (clause 5.2.9) is stored as one JSONB
//! document because that document is the authority for every response body.
//! Discovery asks the opposite question -- "which registrations could serve
//! this entity type, identifier or attribute" -- and no ordinary index answers
//! it over a nested document. `registration_information` is the flattened form
//! that does: one row per selective capability, rewritten by this module inside
//! the same transaction as every registration mutation. Nothing in the database
//! derives it; there is no trigger parsing the document, because a trigger would
//! own domain semantics that belong in the broker.
//!
//! # Superset preselection
//!
//! The projection narrows candidates and decides nothing. The typed evaluation
//! over the registrations it preselects remains the oracle for membership, so
//! every derivation choice below resolves ambiguity towards the *wider* row: a
//! row that admits registrations which turn out not to match costs residual CPU,
//! while a row that omits one that does silently drops a context source from
//! distributed operations.
//!
//! # Sentinels, and the unconstrained registration
//!
//! An axis a row does not constrain is stored as a sentinel rather than as
//! `NULL` -- [`TypeRouting::Any`] is `{'*'}`, [`IdentifierRouting::Any`] is
//! `''`, [`AttributeRouting::Any`] is `{0}` on both key arrays -- so that a
//! reader's test for "declares the requested value or declares everything" is
//! one indexable overlap or membership test naming the sentinel beside the
//! value. `information` entries are disjunctive: a registration serves the union
//! of what its entries declare. An entry that constrains no axis therefore makes
//! the whole registration unconstrained, and so does an empty `information`
//! array. Such a registration is [`RegistrationRouting::Unconstrained`] and is
//! stored as *one wildcard row* carrying every sentinel, so it is a candidate on
//! every dimension without any reader having to test for the absence of rows.
//!
//! # The table this projection is written to
//!
//! ```sql
//! CREATE TABLE registration_information (
//!     id                     BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
//!     registration_id        TEXT COLLATE "C" NOT NULL
//!                                REFERENCES context_source_registrations (id) ON DELETE CASCADE,
//!     entity_types           TEXT[] COLLATE "C" NOT NULL,   -- {'*'} = any
//!     entity_id              TEXT COLLATE "C" NOT NULL,     -- '' = any
//!     entity_id_pattern      TEXT,
//!     property_name_keys      INTEGER[] NOT NULL,            -- {0} = any, {} = none
//!     relationship_name_keys  INTEGER[] NOT NULL             -- {0} = any, {} = none
//! );
//! ```
//!
//! The schema migration that creates `registration_information` declares it,
//! with the storage parameters, the well-formedness check on the sentinels and
//! the index set that belong to the schema rather than to this module; nothing
//! here may name a column that migration does not. This module derives the rows;
//! [`crate::csource_registration::routing_rewrite`] writes them.
//!
//! The row has no natural key. It is never referenced, never updated in place
//! and always replaced as a set, so a surrogate the database hands out is the
//! whole of its identity. The parent key is the registration URN itself rather
//! than a surrogate: registrations are catalogue-scale and each yields a handful
//! of rows, so interning the URN would buy no measurable width while forcing
//! every mutation through an extra round trip to fetch the key.
//!
//! Attribute names are interned through the `attribute_names` dictionary, whose
//! `INTEGER` keys the rest of the schema already indexes on; dictionary keys
//! start at 1, which is what frees `0` to be the sentinel. Entity identifiers
//! and identifier patterns stay text at the persistence boundary: an identifier
//! is matched for equality and a pattern is compiled by PostgreSQL, and neither
//! is a closed vocabulary worth a dictionary. The registration's operations are
//! not projected: operation matching reads the registration's own column, and
//! no preselect filters on them.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{entity_info::EntityInfo, registration::info::RegistrationInfo};
use andromeda_ngsi_ld_query::pattern::Pattern;
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use std::{collections::HashMap, hash::BuildHasher};

/// The type-axis sentinel stored where a row admits every type.
const ANY_TYPE: &str = "*";

/// The identifier-axis sentinel stored where a row names no identifier.
const ANY_IDENTIFIER: &str = "";

/// The attribute-axis sentinel stored on both key arrays where a row declares
/// no name at all.
const ANY_ATTRIBUTE_KEY: i32 = 0;

/// What a row says about the entity-type axis.
#[derive(Debug, PartialEq, Eq)]
pub enum TypeRouting<'info> {
    /// The selector admits every type; stored as `{'*'}`.
    Any,
    /// The type names the `EntityInfo` registers (Table 5.2.8-1: one name or
    /// an array of them). Never empty.
    Named(Vec<&'info EntityTypeName>),
}

impl TypeRouting<'_> {
    /// The `text[]` the row stores.
    pub(crate) fn stored(&self) -> Vec<String> {
        match self {
            TypeRouting::Any => vec![ANY_TYPE.to_owned()],
            TypeRouting::Named(names) => names.iter().map(|name| name.as_str().to_owned()).collect(),
        }
    }
}

/// What a row says about the exact-identifier axis.
#[derive(Debug, PartialEq, Eq)]
pub enum IdentifierRouting<'info> {
    /// The selector names no exact identifier; stored as `''`.
    Any,
    /// The exact identifier the selector names.
    Exact(&'info Identifier),
}

impl IdentifierRouting<'_> {
    /// The text the row stores.
    pub(crate) fn stored(&self) -> &str {
        match self {
            IdentifierRouting::Any => ANY_IDENTIFIER,
            IdentifierRouting::Exact(identifier) => identifier.as_str(),
        }
    }
}

/// What a row says about the two attribute-name axes together.
///
/// The two axes are one value because the writer widens them together: a name
/// on either axis that the dictionary cannot intern makes the whole pair
/// [`AttributeRouting::Any`], never one populated axis beside a widened one --
/// a reader tells "declared nothing here" from "declared something that could
/// not be stored" by nothing but these two columns, and a half-widened pair
/// would read as a complete, narrower declaration than the registration made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeRouting {
    /// The item declares no name on either axis; stored as `{0}` twice.
    Any,
    /// The dictionary keys the item declares. At least one of the two is
    /// non-empty; an axis the item did not declare is stored as `{}`, which
    /// overlaps nothing.
    Declared {
        /// Keys of the item's `propertyNames`.
        properties: Vec<AttributeNameKey>,
        /// Keys of the item's `relationshipNames`.
        relationships: Vec<AttributeNameKey>,
    },
}

impl AttributeRouting {
    /// The two `integer[]` values the row stores, properties first.
    pub(crate) fn stored(&self) -> (Vec<i32>, Vec<i32>) {
        match self {
            AttributeRouting::Any => (vec![ANY_ATTRIBUTE_KEY], vec![ANY_ATTRIBUTE_KEY]),
            AttributeRouting::Declared { properties, relationships } => (
                properties.iter().map(|key| key.get()).collect(),
                relationships.iter().map(|key| key.get()).collect(),
            ),
        }
    }
}

/// One row of the routing projection.
///
/// Every field is a dimension a discovery predicate may narrow on; an axis the
/// row does not constrain carries that axis's sentinel.
#[derive(Debug)]
pub struct RegistrationInformationRow<'info> {
    /// The entity-type axis.
    pub types: TypeRouting<'info>,
    /// The exact-identifier axis.
    pub identifier: IdentifierRouting<'info>,
    /// The identifier pattern the selector carries, if it carries one.
    pub identifier_pattern: Option<&'info Pattern>,
    /// The two attribute-name axes.
    pub attributes: AttributeRouting,
}

impl RegistrationInformationRow<'_> {
    /// The row of a registration that constrains nothing: every sentinel.
    pub(crate) const fn wildcard() -> RegistrationInformationRow<'static> {
        RegistrationInformationRow {
            types: TypeRouting::Any,
            identifier: IdentifierRouting::Any,
            identifier_pattern: None,
            attributes: AttributeRouting::Any,
        }
    }

    /// Whether the row constrains any axis. A row that constrains none would
    /// be a wildcard row, and a wildcard row beside selective siblings means
    /// the whole registration is unconstrained.
    fn is_selective(&self) -> bool {
        self.types != TypeRouting::Any
            || self.identifier != IdentifierRouting::Any
            || self.identifier_pattern.is_some()
            || self.attributes != AttributeRouting::Any
    }
}

/// The projection of one registration's whole `information` document.
///
/// A [`Pattern`] compiles to a regular expression, which has no equality, so
/// this pair of types cannot derive `PartialEq`; tests compare the dimensions
/// they are about.
#[derive(Debug)]
pub enum RegistrationRouting<'info> {
    /// The registration constrains nothing and is a candidate for every
    /// request; stored as one wildcard row.
    Unconstrained,
    /// The registration's selective capabilities, one row each.
    ///
    /// Never empty: an entry that produced no constraint would have made the
    /// whole registration [`RegistrationRouting::Unconstrained`].
    Selective(Vec<RegistrationInformationRow<'info>>),
}

impl<'info> RegistrationRouting<'info> {
    /// The rows to store: the selective rows, or the one wildcard row.
    #[must_use]
    pub fn into_rows(self) -> Vec<RegistrationInformationRow<'info>> {
        match self {
            RegistrationRouting::Unconstrained => vec![RegistrationInformationRow::wildcard()],
            RegistrationRouting::Selective(rows) => rows,
        }
    }
}

/// The attribute names a registration's `information` mentions, deduplicated.
///
/// These are the names the dictionary has to hold keys for before the
/// projection can be written. They arrive already expanded to IRIs: JSON-LD
/// expansion happens in the service layer, above the store.
#[must_use]
pub fn projected_attribute_names(information: &[RegistrationInfo]) -> Vec<AttributeName> {
    let mut names: Vec<AttributeName> = Vec::new();
    for entry in information {
        let declared = entry.property_names.iter().chain(entry.relationship_names.iter()).flatten();
        for name in declared {
            if !names.contains(name) {
                names.push(name.clone()); // clone: the resolver takes an owned, validated name.
            }
        }
    }
    names
}

/// Derive the routing of a registration from its typed `information`.
///
/// `ids` is the dictionary resolution of [`projected_attribute_names`]. A name
/// missing from it widens the entry's whole attribute constraint away -- both
/// the property and the relationship axis -- rather than narrowing a row to the
/// names that did resolve, which would drop a registration that genuinely serves
/// the missing one.
#[must_use]
pub fn derive_routing<'info, S: BuildHasher>(
    information: &'info [RegistrationInfo],
    ids: &HashMap<AttributeName, AttributeNameKey, S>,
) -> RegistrationRouting<'info> {
    let mut rows = Vec::new();
    for entry in information {
        match entry_rows(entry, ids) {
            Some(entry_rows) => rows.extend(entry_rows),
            None => return RegistrationRouting::Unconstrained,
        }
    }
    if rows.is_empty() {
        return RegistrationRouting::Unconstrained;
    }
    RegistrationRouting::Selective(rows)
}

/// The rows one `information` entry contributes, or `None` where the entry
/// constrains no axis and so makes the whole registration unconstrained.
fn entry_rows<'info, S: BuildHasher>(
    entry: &'info RegistrationInfo,
    ids: &HashMap<AttributeName, AttributeNameKey, S>,
) -> Option<Vec<RegistrationInformationRow<'info>>> {
    let attributes = entry_attributes(entry, ids);
    let selectors = entry.entities.as_deref().filter(|selectors| !selectors.is_empty());

    if let Some(selectors) = selectors {
        let rows: Vec<RegistrationInformationRow<'info>> = selectors
            .iter()
            .map(|selector| selector_row(selector, attributes.clone())) // clone: each row owns its own copy of the entry-wide key set.
            .collect();
        // An `EntityInfo` always carries a `type` (Table 5.2.8-1), so a row
        // built from one constrains at least the type axis; the guard is
        // kept so a row that somehow constrains nothing still widens the
        // whole registration rather than hiding behind selective siblings.
        if rows.iter().any(|row| !row.is_selective()) { None } else { Some(rows) }
    } else {
        let row = RegistrationInformationRow {
            types: TypeRouting::Any,
            identifier: IdentifierRouting::Any,
            identifier_pattern: None,
            attributes,
        };
        if row.is_selective() { Some(vec![row]) } else { None }
    }
}

/// One row for one `EntityInfo`, carrying its entry's attribute constraints.
fn selector_row(selector: &EntityInfo, attributes: AttributeRouting) -> RegistrationInformationRow<'_> {
    RegistrationInformationRow {
        types: TypeRouting::Named(selector.type_.names().iter().collect()),
        identifier: selector.id.as_ref().map_or(IdentifierRouting::Any, IdentifierRouting::Exact),
        identifier_pattern: selector.id_pattern.as_ref(),
        attributes,
    }
}

/// The dictionary resolution of one declared attribute-name list.
#[derive(Debug)]
enum DeclaredNames {
    /// The entry declared no name on this axis.
    Absent,
    /// Every declared name resolved to a dictionary key.
    Interned(Vec<AttributeNameKey>),
    /// At least one declared name is not in the dictionary.
    Unresolved,
}

/// The attribute routing of an entry's `propertyNames` and `relationshipNames`.
fn entry_attributes<S: BuildHasher>(entry: &RegistrationInfo, ids: &HashMap<AttributeName, AttributeNameKey, S>) -> AttributeRouting {
    let properties = interned_names(entry.property_names.as_deref(), ids);
    let relationships = interned_names(entry.relationship_names.as_deref(), ids);
    match (properties, relationships) {
        (DeclaredNames::Unresolved, _) | (_, DeclaredNames::Unresolved) | (DeclaredNames::Absent, DeclaredNames::Absent) => AttributeRouting::Any,
        (properties, relationships) => AttributeRouting::Declared {
            properties: declared_keys(properties),
            relationships: declared_keys(relationships),
        },
    }
}

/// The keys one resolved axis stores beside a declared sibling axis: an absent
/// axis is the empty set, which overlaps nothing.
fn declared_keys(names: DeclaredNames) -> Vec<AttributeNameKey> {
    match names {
        DeclaredNames::Absent | DeclaredNames::Unresolved => Vec::new(),
        DeclaredNames::Interned(keys) => keys,
    }
}

/// The dictionary keys of a declared attribute-name list.
fn interned_names<S: BuildHasher>(names: Option<&[AttributeName]>, ids: &HashMap<AttributeName, AttributeNameKey, S>) -> DeclaredNames {
    match names.filter(|names| !names.is_empty()) {
        None => DeclaredNames::Absent,
        Some(names) => match names.iter().map(|name| ids.get(name).copied()).collect() {
            Some(keys) => DeclaredNames::Interned(keys),
            None => DeclaredNames::Unresolved,
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::csource_registration::information_projection::{
        AttributeRouting,
        IdentifierRouting,
        RegistrationInformationRow,
        RegistrationRouting,
        TypeRouting,
        derive_routing,
        projected_attribute_names,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
    use andromeda_ngsi_ld_model::{
        entity_info::{EntityInfo, EntityTypeSelection},
        registration::info::RegistrationInfo,
    };
    use andromeda_ngsi_ld_query::pattern::Pattern;
    use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    /// An expanded attribute IRI, as the service layer hands one to the store.
    const SPEED: &str = "https://example.com/speed";
    /// A second expanded attribute IRI.
    const DRIVER: &str = "https://example.com/driver";

    fn name(iri: &str) -> AttributeName {
        iri.parse().expect("valid attribute name")
    }

    fn dictionary() -> HashMap<AttributeName, AttributeNameKey> {
        HashMap::from([(name(SPEED), AttributeNameKey::from(11)), (name(DRIVER), AttributeNameKey::from(12))])
    }

    fn single_type(raw_type: &str) -> EntityTypeSelection {
        EntityTypeSelection::Single(raw_type.parse().expect("valid type name"))
    }

    fn selector(raw_type: &str) -> EntityInfo {
        EntityInfo {
            id: None,
            id_pattern: None,
            type_: single_type(raw_type),
        }
    }

    fn entry(entities: Option<Vec<EntityInfo>>, properties: Option<Vec<&str>>, relationships: Option<Vec<&str>>) -> RegistrationInfo {
        RegistrationInfo {
            entities,
            property_names: properties.map(|iris| iris.into_iter().map(name).collect()),
            relationship_names: relationships.map(|iris| iris.into_iter().map(name).collect()),
        }
    }

    /// Whether a derivation left the registration matching every request.
    fn unconstrained(routing: &RegistrationRouting<'_>) -> bool {
        matches!(routing, RegistrationRouting::Unconstrained)
    }

    fn rows_of<'info>(information: &'info [RegistrationInfo], ids: &HashMap<AttributeName, AttributeNameKey>) -> Vec<RegistrationInformationRow<'info>> {
        match derive_routing(information, ids) {
            RegistrationRouting::Unconstrained => panic!("the fixture constrains at least one axis"),
            RegistrationRouting::Selective(rows) => rows,
        }
    }

    fn keys(ids: &[i32]) -> Vec<AttributeNameKey> {
        ids.iter().copied().map(AttributeNameKey::from).collect()
    }

    fn type_names<'info>(row: &RegistrationInformationRow<'info>) -> Vec<&'info str> {
        match &row.types {
            TypeRouting::Any => panic!("the fixture names a type"),
            TypeRouting::Named(names) => names.iter().map(|name| name.as_str()).collect(),
        }
    }

    #[test]
    fn a_type_only_selector_projects_one_row_constraining_the_type_alone() {
        let information = vec![entry(Some(vec![selector("Vehicle")]), None, None)];
        let rows = rows_of(&information, &dictionary());
        assert_eq!(rows.len(), 1);
        assert_eq!(type_names(&rows[0]), vec!["Vehicle"]);
        assert_eq!(rows[0].identifier, IdentifierRouting::Any);
        assert!(rows[0].identifier_pattern.is_none());
        assert_eq!(rows[0].attributes, AttributeRouting::Any);
    }

    #[test]
    fn an_exact_identifier_selector_projects_the_identifier_alongside_its_type() {
        let identifier: Identifier = "urn:ngsi-ld:Vehicle:A1".parse().expect("valid URI");
        let information = vec![entry(
            Some(vec![EntityInfo {
                id: Some(identifier),
                id_pattern: None,
                type_: single_type("Vehicle"),
            }]),
            None,
            None,
        )];
        let rows = rows_of(&information, &dictionary());
        assert_eq!(
            rows[0].identifier,
            IdentifierRouting::Exact(&"urn:ngsi-ld:Vehicle:A1".parse().expect("valid URI"))
        );
        assert_eq!(type_names(&rows[0]), vec!["Vehicle"]);
    }

    #[test]
    fn an_identifier_pattern_selector_projects_the_pattern_as_written() {
        let information = vec![entry(
            Some(vec![EntityInfo {
                id: None,
                id_pattern: Some(Pattern::parse("^urn:ngsi-ld:Vehicle:.*$").expect("valid pattern")),
                type_: single_type("Vehicle"),
            }]),
            None,
            None,
        )];
        let rows = rows_of(&information, &dictionary());
        assert_eq!(rows[0].identifier_pattern.map(Pattern::as_str), Some("^urn:ngsi-ld:Vehicle:.*$"));
        assert_eq!(rows[0].identifier, IdentifierRouting::Any, "a pattern selector names no exact identifier");
    }

    #[test]
    fn declared_property_and_relationship_names_project_as_dictionary_keys() {
        let information = vec![entry(None, Some(vec![SPEED]), Some(vec![DRIVER]))];
        let rows = rows_of(&information, &dictionary());
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].attributes,
            AttributeRouting::Declared {
                properties: keys(&[11]),
                relationships: keys(&[12])
            }
        );
        assert_eq!(rows[0].types, TypeRouting::Any, "an entry without entities constrains no entity axis");
    }

    /// An axis the entry did not declare beside one it did stores the empty
    /// set, not the sentinel: the sentinel would admit every attribute query.
    #[test]
    fn a_name_declared_on_one_axis_alone_leaves_the_other_axis_empty_rather_than_wildcarded() {
        let information = vec![entry(None, Some(vec![SPEED]), None)];
        let rows = rows_of(&information, &dictionary());
        assert_eq!(
            rows[0].attributes,
            AttributeRouting::Declared {
                properties: keys(&[11]),
                relationships: Vec::new()
            }
        );
    }

    #[test]
    fn an_entry_combining_selectors_and_names_repeats_the_names_on_every_selector_row() {
        let information = vec![entry(Some(vec![selector("Vehicle"), selector("Building")]), Some(vec![SPEED]), None)];
        let rows = rows_of(&information, &dictionary());
        assert_eq!(rows.len(), 2);
        for row in &rows {
            assert_eq!(
                row.attributes,
                AttributeRouting::Declared {
                    properties: keys(&[11]),
                    relationships: Vec::new()
                }
            );
        }
    }

    #[test]
    fn several_information_entries_each_contribute_their_own_rows() {
        let information = vec![entry(Some(vec![selector("Vehicle")]), None, None), entry(None, Some(vec![SPEED, DRIVER]), None)];
        let rows = rows_of(&information, &dictionary());
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[1].attributes,
            AttributeRouting::Declared {
                properties: keys(&[11, 12]),
                relationships: Vec::new()
            }
        );
    }

    /// Table 5.2.8-1: an `EntityInfo` may name several types; the row lists
    /// them all and a request for any of them preselects the registration.
    #[test]
    fn a_type_array_projects_every_name_it_carries() {
        let several = EntityInfo {
            id: None,
            id_pattern: None,
            type_: EntityTypeSelection::Several(vec!["Building".parse().expect("valid type name"), "Tower".parse().expect("valid type name")]),
        };
        let information = vec![entry(Some(vec![several]), None, None)];
        let rows = rows_of(&information, &dictionary());
        assert_eq!(type_names(&rows[0]), vec!["Building", "Tower"]);
    }

    /// Entries are disjunctive: one entry that constrains nothing makes the
    /// whole registration a candidate for every request, so the selective
    /// entries beside it must not be projected as if they bounded it.
    #[test]
    fn an_unconstrained_entry_suppresses_the_rows_of_its_selective_siblings() {
        let information = vec![entry(Some(vec![selector("Vehicle")]), None, None), entry(None, None, None)];
        assert!(unconstrained(&derive_routing(&information, &dictionary())));
    }

    #[test]
    fn a_registration_without_information_is_unconstrained() {
        assert!(unconstrained(&derive_routing(&[], &dictionary())));
    }

    /// The unconstrained registration is stored as one row carrying every
    /// sentinel, so a reader never has to test for the absence of rows.
    #[test]
    fn an_unconstrained_registration_stores_one_wildcard_row() {
        let rows = derive_routing(&[], &dictionary()).into_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].types, TypeRouting::Any);
        assert_eq!(rows[0].identifier, IdentifierRouting::Any);
        assert!(rows[0].identifier_pattern.is_none());
        assert_eq!(rows[0].attributes, AttributeRouting::Any);
    }

    /// An empty `entities` array bounds nothing, exactly as an absent one does.
    #[test]
    fn an_empty_entity_selector_list_constrains_no_entity_axis() {
        let information = vec![entry(Some(Vec::new()), Some(vec![SPEED]), None)];
        let rows = rows_of(&information, &dictionary());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].types, TypeRouting::Any);
        assert_eq!(
            rows[0].attributes,
            AttributeRouting::Declared {
                properties: keys(&[11]),
                relationships: Vec::new()
            }
        );
    }

    /// A name the dictionary does not hold widens the attribute axis instead of
    /// narrowing it to the names that did resolve: preselection may return a
    /// registration that does not match, never omit one that does.
    #[test]
    fn an_unresolved_attribute_name_widens_the_attribute_axes_rather_than_narrowing_them() {
        let information = vec![entry(Some(vec![selector("Vehicle")]), Some(vec![SPEED, "https://example.com/unknown"]), None)];
        let rows = rows_of(&information, &dictionary());
        assert_eq!(rows[0].attributes, AttributeRouting::Any);
        assert_eq!(type_names(&rows[0]), vec!["Vehicle"], "the entity axis is unaffected");
    }

    /// An unresolved name on one axis widens the other one with it, so the pair
    /// of columns never reads as a complete declaration narrower than the one
    /// the registration made.
    #[test]
    fn an_unresolved_property_name_widens_the_relationship_axis_as_well() {
        let information = vec![entry(
            Some(vec![selector("Vehicle")]),
            Some(vec![SPEED, "https://example.com/unknown"]),
            Some(vec![DRIVER]),
        )];
        let rows = rows_of(&information, &dictionary());
        assert_eq!(rows[0].attributes, AttributeRouting::Any);
    }

    /// The same widening, applied to an entry whose only constraint was the
    /// unresolved name.
    #[test]
    fn an_entry_constrained_only_by_an_unresolved_name_makes_the_registration_unconstrained() {
        let information = vec![entry(None, Some(vec!["https://example.com/unknown"]), None)];
        assert!(unconstrained(&derive_routing(&information, &dictionary())));
    }

    #[test]
    fn the_names_to_intern_are_every_declared_name_without_repetition() {
        let information = vec![entry(None, Some(vec![SPEED]), Some(vec![DRIVER])), entry(None, Some(vec![SPEED]), None)];
        assert_eq!(projected_attribute_names(&information), vec![name(SPEED), name(DRIVER)]);
    }

    #[test]
    fn a_registration_without_declared_names_interns_nothing() {
        let information = vec![entry(Some(vec![selector("Vehicle")]), None, None)];
        assert_eq!(projected_attribute_names(&information), [] as [AttributeName; 0]);
    }
}

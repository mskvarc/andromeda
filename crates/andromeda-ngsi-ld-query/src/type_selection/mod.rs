//! The NGSI-LD Entity Type Selection Language (clause 4.17).
//!
//! The `type` query parameter carries a type selection: a boolean combination
//! of entity type names. This module owns its grammar, its parser, its
//! in-memory evaluation and the expansion of compact names to IRIs; rendering
//! the expanded tree against a store belongs to that backend.
//!
//! One type, [`TypeSelection`], holds both name stages. `expand_type_selection`
//! turns a `TypeSelection<Written>` into a `TypeSelection<Expanded>` through the
//! request `@context`, `compact_type_selection` spells it back leaf by leaf in a
//! reader's terms, and `TypeSelection::into_expanded` checks a stored text whose
//! types are already absolute IRIs into the expanded stage.

pub mod compaction;
pub mod expansion;
mod parser;
pub mod type_alternatives;
pub mod type_selection_error;

use crate::{
    name_stage::{Expanded, NameStage, Written},
    query_error::QueryError,
    term_expander::TermExpansionError,
    type_selection::{type_alternatives::TypeAlternatives, type_selection_error::TypeSelectionError},
};
use andromeda_ngsi_ld_lexicon::{entity_type_iri::EntityTypeIri, entity_type_name::EntityTypeName};
use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};

/// An entity type selection (clause 4.17): the wildcard, or a disjunction of
/// alternatives, each one entity type or a conjunction of them.
///
/// The tree is generic over the [`NameStage`] of its types: a
/// `TypeSelection<Written>` is what the parser reads and what a stored resource
/// carries (clause 5.2.33 puts a `type` in an `EntitySelector` before any
/// `@context` has been applied to it), and a `TypeSelection<Expanded>` is what
/// expansion through the request `@context` makes and what an execution reads.
///
/// The wire form is the clause 4.17 text itself, in both stages, so a
/// subscription that selected `Building|Tower` gives back an equivalent
/// selection rather than a regex someone rebuilt from it. The text of an
/// expanded selection is read as written and then checked to hold only absolute
/// IRIs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(
    try_from = "String",
    into = "String",
    bound(
        serialize = "S: NameStage",
        deserialize = "S: NameStage, TypeSelection<S>: TryFrom<String>, <TypeSelection<S> as TryFrom<String>>::Error: Display"
    )
)]
pub enum TypeSelection<S: NameStage> {
    /// The clause 4.17 wildcard, selecting every entity type.
    Wildcard,
    /// The entity types of at least one alternative.
    Alternatives(TypeAlternatives<S>),
}

impl<S: NameStage> TypeSelection<S> {
    /// The selection of the one type `entity_type`.
    #[must_use]
    pub fn single(entity_type: S::EntityType) -> TypeSelection<S> {
        TypeSelection::Alternatives(TypeAlternatives::single(entity_type))
    }

    /// The selection of the alternatives `alternatives`, each a conjunction, or
    /// `None` when there is none or one of them holds no type.
    #[must_use]
    pub fn of_alternatives(alternatives: Vec<Vec<S::EntityType>>) -> Option<TypeSelection<S>> {
        TypeAlternatives::new(alternatives).map(TypeSelection::Alternatives)
    }

    /// Whether a set of entity type names satisfies this selection.
    ///
    /// Names are compared whole, so the selector `Building` does not match an
    /// entity typed `MyBuilding`. A caller that has the expanded payload has
    /// IRIs and nothing to compact them against, so the leaves are compared as
    /// the text they spell.
    pub fn matches<T: AsRef<str>>(&self, types: &[T]) -> bool {
        match self {
            TypeSelection::Wildcard => true,
            TypeSelection::Alternatives(alternatives) => alternatives.matches(types),
        }
    }

    /// The single type this selection selects, when it selects exactly one.
    /// The wildcard and a boolean combination name none.
    #[must_use]
    pub const fn as_single_type(&self) -> Option<&S::EntityType> {
        match self {
            TypeSelection::Wildcard => None,
            TypeSelection::Alternatives(alternatives) => alternatives.as_single_type(),
        }
    }

    /// Types of which every Entity the selection selects carries one, each
    /// once ([`TypeAlternatives::covering_types`]), `None` for the wildcard,
    /// which selects Entities of every type.
    ///
    /// A caller that indexes subscriptions by type files a selection under
    /// these: a single type under itself, and a boolean combination under one
    /// type of each of its alternatives.
    #[must_use]
    pub fn covering_types(&self) -> Option<Vec<&S::EntityType>> {
        match self {
            TypeSelection::Wildcard => None,
            TypeSelection::Alternatives(alternatives) => Some(alternatives.covering_types()),
        }
    }

    /// The types the selection names, in order of appearance.
    ///
    /// The wildcard names none. A caller that must hand a flat list of types to
    /// code that does not read the boolean structure uses this, knowing it
    /// over-approximates a conjunction.
    #[must_use]
    pub fn named_types(&self) -> Vec<S::EntityType> {
        match self {
            TypeSelection::Wildcard => Vec::new(),
            TypeSelection::Alternatives(alternatives) => alternatives.leaves().cloned().collect(),
        }
    }

    /// The selection of every selection in `selections`, as clause 4.17 reads a
    /// top-level disjunction: none for no selection, the selection itself for
    /// one, and the wildcard when any of them is the wildcard.
    #[must_use]
    pub fn any_of(selections: impl IntoIterator<Item = TypeSelection<S>>) -> Option<TypeSelection<S>> {
        let mut alternatives = Vec::new();
        for selection in selections {
            match selection {
                TypeSelection::Wildcard => return Some(TypeSelection::Wildcard),
                TypeSelection::Alternatives(selected) => alternatives.push(selected),
            }
        }
        TypeAlternatives::union(alternatives).map(TypeSelection::Alternatives)
    }
}

impl TypeSelection<Written> {
    /// The selection a stored resource holds, read as the IRIs its leaves
    /// already are.
    ///
    /// A resource stores its type selections expanded, so every leaf of one
    /// read back is the absolute IRI the request's `@context` expanded it to.
    ///
    /// # Errors
    ///
    /// Returns [`TermExpansionError::NotAnIri`] when a leaf is no absolute IRI,
    /// which a selection expanded before it was stored never holds.
    pub fn into_expanded(self) -> Result<TypeSelection<Expanded>, TermExpansionError> {
        match self {
            TypeSelection::Wildcard => Ok(TypeSelection::Wildcard),
            TypeSelection::Alternatives(alternatives) => alternatives
                .try_map(|name| EntityTypeIri::from_str(name.as_str()).map_err(|_| TermExpansionError::NotAnIri))
                .map(TypeSelection::Alternatives),
        }
    }
}

impl TypeSelection<Expanded> {
    /// The selection as written text over the IRIs it holds: the inverse of
    /// [`TypeSelection::into_expanded`].
    ///
    /// Full IRIs satisfy the same clause 4.6.2 name grammar as compact type
    /// names. This conversion lets a stored resource retain the clause 4.17
    /// wire shape after every leaf has crossed the JSON-LD expansion boundary.
    ///
    /// # Errors
    ///
    /// Returns [`TypeSelectionError::InvalidName`] when a leaf IRI does not
    /// satisfy the clause 4.6.2 type-name grammar, which includes the identifier
    /// byte bound that an expanded type does not carry itself.
    pub fn into_written(self) -> Result<TypeSelection<Written>, TypeSelectionError> {
        match self {
            TypeSelection::Wildcard => Ok(TypeSelection::Wildcard),
            TypeSelection::Alternatives(alternatives) => alternatives
                .try_map(|iri| {
                    iri.as_str()
                        .parse::<EntityTypeName>()
                        .map_err(|source| TypeSelectionError::InvalidName { position: 0, source })
                })
                .map(TypeSelection::Alternatives),
        }
    }

    /// Check that every leaf IRI satisfies the clause 4.6.2 type-name grammar,
    /// the identifier byte bound included, which a stored selection keeps its
    /// types within.
    ///
    /// # Errors
    ///
    /// Returns [`TypeSelectionError::InvalidName`] for the first leaf that does
    /// not.
    pub fn check_written_bound(&self) -> Result<(), TypeSelectionError> {
        match self {
            TypeSelection::Wildcard => Ok(()),
            TypeSelection::Alternatives(alternatives) => alternatives.leaves().try_for_each(|iri| {
                iri.as_str()
                    .parse::<EntityTypeName>()
                    .map(drop)
                    .map_err(|source| TypeSelectionError::InvalidName { position: 0, source })
            }),
        }
    }
}

impl FromStr for TypeSelection<Written> {
    type Err = TypeSelectionError;

    fn from_str(raw: &str) -> Result<TypeSelection<Written>, TypeSelectionError> {
        parser::parse(raw)
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this
// adapter exists only because `#[serde(try_from = "String")]` requires it.
impl TryFrom<String> for TypeSelection<Written> {
    type Error = TypeSelectionError;

    fn try_from(raw: String) -> Result<TypeSelection<Written>, TypeSelectionError> {
        raw.parse()
    }
}

// Hand-written: the text of an expanded selection is parsed as the written
// stage and then checked, which no derive expresses.
impl FromStr for TypeSelection<Expanded> {
    type Err = QueryError;

    fn from_str(raw: &str) -> Result<TypeSelection<Expanded>, QueryError> {
        Ok(raw.parse::<TypeSelection<Written>>()?.into_expanded()?)
    }
}

// Hand-written: the text of an expanded selection is parsed as the written
// stage and then checked, which no derive expresses.
impl TryFrom<String> for TypeSelection<Expanded> {
    type Error = QueryError;

    fn try_from(raw: String) -> Result<TypeSelection<Expanded>, QueryError> {
        Ok(TypeSelection::<Written>::try_from(raw)?.into_expanded()?)
    }
}

// The wire form is produced by the `Display` below; this exists only because
// `#[serde(into = "String")]` demands an `Into<String>` and cannot use it.
impl<S: NameStage> From<TypeSelection<S>> for String {
    fn from(type_selection: TypeSelection<S>) -> String {
        type_selection.to_string()
    }
}

// Hand-written: the wildcard renders as `*` and the alternatives as their own
// text, which a derive cannot express.
impl<S: NameStage> Display for TypeSelection<S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TypeSelection::Wildcard => formatter.write_str("*"),
            TypeSelection::Alternatives(alternatives) => alternatives.fmt(formatter),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        name_stage::{Expanded, Written},
        term_expander::TermExpansionError,
        type_selection::TypeSelection,
    };
    use andromeda_ngsi_ld_lexicon::{entity_type_iri::EntityTypeIri, entity_type_name::EntityTypeName, identifier_length::IDENTIFIER_BYTE_LIMIT};

    fn written(raw: &str) -> TypeSelection<Written> {
        raw.parse().expect("valid type expression")
    }

    fn expanded(raw: &str) -> TypeSelection<Expanded> {
        written(raw).into_expanded().expect("absolute IRIs")
    }

    #[test]
    fn a_type_selection_round_trips_through_its_clause_4_17_text() {
        for raw in ["Building", "Building,Tower", "(Building;Vehicle)", "(Home;Vehicle),Motorhome", "*"] {
            let parsed = written(raw);
            let reparsed = written(&parsed.to_string());
            pretty_assertions::assert_eq!(parsed, reparsed, "{raw}");
        }
    }

    #[test]
    fn a_stored_expanded_type_selection_round_trips_through_its_text() {
        for raw in [
            "https://example.com/Home",
            "https://example.com/Building,https://example.com/Tower",
            "(https://example.com/Home;https://example.com/Vehicle),https://example.com/Motorhome",
            "*",
        ] {
            let selection = expanded(raw);
            pretty_assertions::assert_eq!(selection.to_string(), raw);
            let json = serde_json::to_string(&selection).expect("serialises");
            pretty_assertions::assert_eq!(
                serde_json::from_str::<TypeSelection<Expanded>>(&json).expect("deserialises"),
                selection,
                "{raw}"
            );
        }
    }

    #[test]
    fn an_expanded_type_selection_refuses_a_bare_term_when_deserialised() {
        assert!(serde_json::from_str::<TypeSelection<Expanded>>("\"Vehicle\"").is_err());
        assert!(serde_json::from_str::<TypeSelection<Expanded>>("\"https://example.com/Home,Vehicle\"").is_err());
    }

    /// A leaf that is no absolute IRI was never expanded, so it is refused
    /// rather than read as an IRI it does not name.
    #[test]
    fn a_compact_leaf_is_no_expanded_type_selection() {
        pretty_assertions::assert_eq!(written("Vehicle").into_expanded(), Err(TermExpansionError::NotAnIri));
        pretty_assertions::assert_eq!(written("https://example.com/Home,Vehicle").into_expanded(), Err(TermExpansionError::NotAnIri));
    }

    #[test]
    fn a_stored_type_selection_reads_back_as_the_expanded_type_selection_it_was_written_from() {
        let selection = expanded("(https://example.com/Home;https://example.com/Vehicle),https://example.com/Motorhome");
        let stored = selection.clone().into_written().expect("every leaf is a type name");
        pretty_assertions::assert_eq!(stored.into_expanded().expect("every leaf is an IRI"), selection);
    }

    #[test]
    fn an_expanded_leaf_past_the_identifier_bound_is_no_written_type_selection() {
        let prefix = "https://example.com/";
        let long = format!("{prefix}{}", "a".repeat(IDENTIFIER_BYTE_LIMIT + 1 - prefix.len()));
        let selection = TypeSelection::<Expanded>::single(long.parse::<EntityTypeIri>().expect("an expanded type carries no length bound"));
        assert!(selection.into_written().is_err());
    }

    #[test]
    fn an_alternation_matches_whole_names_only() {
        let type_selection = written("Building|Tower");
        assert!(type_selection.matches(&["Building"]));
        assert!(type_selection.matches(&["Tower"]));
        assert!(!type_selection.matches(&["MyBuilding"]));
        assert!(!type_selection.matches(&["Vehicle"]));
    }

    #[test]
    fn an_expanded_alternation_matches_whole_iris_only() {
        let type_selection = expanded("https://example.com/Building|https://example.com/Tower");
        assert!(type_selection.matches(&["https://example.com/Tower"]));
        assert!(!type_selection.matches(&["https://example.com/MyBuilding"]));
        assert!(!type_selection.matches(&["https://example.com/Vehicle"]));
    }

    #[test]
    fn the_wildcard_matches_every_set_of_types() {
        assert!(TypeSelection::<Written>::Wildcard.matches::<&str>(&[]));
        assert!(TypeSelection::<Written>::Wildcard.matches(&["Vehicle"]));
    }

    /// Clause 4.17: `OrEntityType = "(" EntityType *(";" EntityType) ")" /
    /// EntityType`, so a conjunction always renders parenthesised, while a
    /// top-level disjunction stays bare.
    #[test]
    fn a_conjunction_always_renders_parenthesised_per_clause_4_17() {
        let shapes = [
            ("Building", "Building"),
            ("Building,Tower", "Building,Tower"),
            ("Building|Tower", "Building,Tower"),
            ("(Home;Vehicle)", "(Home;Vehicle)"),
            ("(Home;Vehicle),Motorhome", "(Home;Vehicle),Motorhome"),
            ("Motorhome,(Home;Vehicle)", "Motorhome,(Home;Vehicle)"),
        ];
        for (input, expected) in shapes {
            let parsed = written(input);
            let rendered = parsed.to_string();
            pretty_assertions::assert_eq!(rendered, expected, "rendering of {input}");
            pretty_assertions::assert_eq!(written(&rendered), parsed, "round trip of {input}");
        }
    }

    #[test]
    fn a_conjunction_needs_every_type_present() {
        let type_selection = written("(Home;Vehicle)");
        assert!(type_selection.matches(&["Home", "Vehicle"]));
        assert!(!type_selection.matches(&["Home"]));
    }

    #[test]
    fn any_of_no_selection_is_none_and_of_one_is_that_selection() {
        pretty_assertions::assert_eq!(TypeSelection::<Written>::any_of(Vec::new()), None);
        pretty_assertions::assert_eq!(TypeSelection::any_of([written("(Home;Vehicle)")]), Some(written("(Home;Vehicle)")));
        pretty_assertions::assert_eq!(
            TypeSelection::any_of([written("Building"), written("(Home;Vehicle)"), written("Tower")]),
            Some(written("Building|(Home;Vehicle)|Tower"))
        );
    }

    #[test]
    fn a_disjunction_holding_the_wildcard_selects_every_type() {
        pretty_assertions::assert_eq!(TypeSelection::any_of([written("Building"), written("*")]), Some(TypeSelection::Wildcard));
    }

    #[test]
    fn a_written_type_selection_names_its_leaf_types_in_order() {
        let name = |raw: &str| raw.parse::<EntityTypeName>().expect("valid type name");
        pretty_assertions::assert_eq!(
            written("Vehicle,(Home;Asset)").named_types(),
            vec![name("Vehicle"), name("Home"), name("Asset")]
        );
        pretty_assertions::assert_eq!(TypeSelection::<Written>::Wildcard.named_types(), Vec::<EntityTypeName>::new());
    }

    #[test]
    fn only_a_leaf_names_a_single_type() {
        assert_eq!(written("Building").as_single_type().map(EntityTypeName::as_str), Some("Building"));
        assert_eq!(written("A,B").as_single_type(), None);
        assert_eq!(written("(A;B)").as_single_type(), None);
        assert_eq!(TypeSelection::<Written>::Wildcard.as_single_type(), None);
    }

    #[test]
    fn a_selection_is_covered_by_one_type_of_each_alternative_and_the_wildcard_by_none() {
        let covering = |text: &str| {
            written(text)
                .covering_types()
                .map(|types| types.into_iter().map(|entity_type| entity_type.as_str().to_owned()).collect::<Vec<_>>())
        };
        assert_eq!(covering("Building"), Some(vec!["Building".to_owned()]));
        assert_eq!(covering("Building|Tower"), Some(vec!["Building".to_owned(), "Tower".to_owned()]));
        assert_eq!(covering("(Home;Vehicle)"), Some(vec!["Home".to_owned()]));
        assert_eq!(TypeSelection::<Written>::Wildcard.covering_types(), None);
    }

    #[test]
    fn a_malformed_type_selection_is_rejected_when_the_resource_is_deserialized() {
        assert!(serde_json::from_str::<TypeSelection<Written>>("\"(Building\"").is_err());
    }
}

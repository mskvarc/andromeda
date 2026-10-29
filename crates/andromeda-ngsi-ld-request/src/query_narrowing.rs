//! The "too wide query" rule the discovery operations share.
//!
//! ETSI GS CIM 009 v1.9.1 states one rule four times, in the same words each
//! time. Clauses 5.6.21.4 (Purge Entities), 5.7.2.4 (Query Entities), 5.7.4.4
//! (Query Temporal Evolution of Entities) and 5.14.4.4 (Create EntityMap):
//! "At least one of the following input data shall be provided: a) selector of
//! Entity Types; b) list of Attribute names, including at least one non-system
//! Attribute; c) NGSI-LD Query, including at least one non-system Attribute;
//! d) NGSI-LD `GeoQuery`; e) local scope (see clause 5.5.13). If none of the
//! above is provided, then an error of type `BadRequestData` shall be raised
//! (too wide query)."
//!
//! What the list leaves out is as load-bearing as what it holds, and the prose
//! above each behaviour clause says why: "it is not possible to retrieve a set
//! of entities by only specifying desired Entity identifiers, without further
//! specifying restrictions on the entities' types or attributes". `id`,
//! `idPattern` and `scopeQ` are therefore not narrowings: a request carrying
//! only them is still too wide. Nor are `pick` and `omit`: clause 5.7.2.3 lists
//! them as projections, apart from the deprecated "list (one or more) of
//! Attribute names of which at least one shall exist in order for an Entity to
//! be selected", which is input (b). Table 6.4.3.2-1 calls `attrs` "a synonym
//! for a combination of the pick and q parameters", and the selecting half of
//! that combination is the `q`, not the `pick`. Purge Entities counts two
//! spellings of input (b): clause 5.6.21.3 gives it `drop` and `keep`, the lists
//! of Attribute names the purge acts on, and Table 6.4.3.3-1 names `attrs` in
//! the rule its `type`, `q` and `geometry` rows restate.
//!
//! Input (a) reaches a POST binding differently: clause 5.2.23's body carries
//! one clause 5.2.11 selector per entry of `entities`, each with its own type
//! selector, which a single flat parameter cannot express. Both spellings are
//! read.
//!
//! Clause 5.10.2.4 (Query Context Source Registrations) states the same rule
//! with two differences: it lists only the four content narrowings, because a
//! registration discovery has no execution to limit to the local scope, and it
//! drops the "non-system Attribute" qualification from (b) and (c).

use crate::{error::ParameterError, query::entity_selector::EntitySelectorParams};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time_prop::TimeProp};
use andromeda_ngsi_ld_query::{
    filter::{expression::Expr, referenced_attributes::referenced_attributes},
    geo::input::GeoQueryInput,
    type_selection::grammar::TypeExprRaw,
};
use strum::{Display, IntoEnumIterator};
use typed_builder::TypedBuilder;

/// Whether the operation's clause lists the local scope among its narrowings.
///
/// Renders as the inputs the operation admits, which is what a too-wide-query
/// refusal names.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum LocalScopeNarrowing {
    /// Input (e) of clauses 5.6.21.4, 5.7.2.4, 5.7.4.4 and 5.14.4.4.
    #[strum(serialize = "type, attrs, q, georel or local=true")]
    Listed,
    /// Clause 5.10.2.4 lists only the four content narrowings.
    #[strum(serialize = "type, attrs, q or georel")]
    Unlisted,
}

/// Whether the operation's clause counts a system Attribute as a narrowing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SystemAttributeNarrowing {
    /// The entity-facing clauses qualify inputs (b) and (c) with "including at
    /// least one non-system Attribute".
    Discounted,
    /// Clause 5.10.2.4 states inputs (b) and (c) without that qualification.
    Counted,
}

/// The narrowing inputs one request carries, and which of them its clause lists.
#[derive(Debug, TypedBuilder)]
pub struct NarrowingInputs<'a> {
    /// Which inputs the operation's own clause enumerates.
    admitted: AdmittedNarrowings,
    /// Input (a): the clause 4.17 selector of Entity Types, as a flat
    /// parameter.
    #[builder(default = Default::default())]
    entity_type: Option<&'a TypeExprRaw>,
    /// Input (a) as a POST body writes it: one clause 5.2.11 selector per
    /// entry, each carrying its own type selector (clause 5.2.23).
    #[builder(default = Default::default())]
    entity_selectors: &'a [EntitySelectorParams],
    /// Input (b): the list of Attribute names, in its deprecated spelling.
    #[builder(default = Default::default())]
    attrs: Option<&'a [AttributeName]>,
    /// Input (b) as Purge Entities writes it. Clause 5.6.21.3 gives the
    /// operation no `attrs`: its lists of Attribute names are "a restrictive
    /// list of Attribute names to be deleted" and "an exclusionary list
    /// Attribute names to be kept", which is what input (b) names there.
    #[builder(default = Default::default())]
    purged: Option<&'a [AttributeName]>,
    /// Input (c): the clause 4.9 NGSI-LD Query.
    #[builder(default = Default::default())]
    q: Option<&'a Expr>,
    /// Input (d): the clause 4.10 NGSI-LD `GeoQuery`.
    #[builder(default = Default::default())]
    geo: Option<&'a GeoQueryInput>,
    /// Input (e): whether execution is limited to the local scope.
    #[builder(default = ExecutionScope::Distributed)]
    local_scope: ExecutionScope,
}

/// Whether a request limits its execution to this broker (clause 5.5.13).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ExecutionScope {
    /// The request may reach the registered Context Sources.
    #[default]
    Distributed,
    /// `local=true`, or a clause 4.17 `*` type selector, which Table 5.2.33-1
    /// defines as implying local scope.
    Local,
}

impl ExecutionScope {
    /// The scope a request's `local` and clause 4.17 type selectors ask for,
    /// the flat `type` and the `type` of each clause 5.2.33 selector of a POST
    /// body alike.
    #[must_use]
    pub fn of(local: Option<bool>, entity_type: Option<&TypeExprRaw>, selectors: &[EntitySelectorParams]) -> ExecutionScope {
        if selects_every_type(entity_type, selectors) {
            ExecutionScope::Local
        } else {
            ExecutionScope::of_flag(local)
        }
    }

    /// The scope a request's `local` flag alone asks for, on an operation
    /// that selects no entities by type (clause 6.3.18).
    #[must_use]
    pub const fn of_flag(local: Option<bool>) -> ExecutionScope {
        match local {
            Some(true) => ExecutionScope::Local,
            Some(false) | None => ExecutionScope::Distributed,
        }
    }
}

/// Refuse a `*` type selector beside `local=false`: Table 5.2.33-1 has `*`
/// imply local scope, which "shall not be explicitly set to false". One `*`
/// among the selectors of a POST body confines the whole request.
///
/// # Errors
///
/// Returns [`ParameterError::WildcardTypeWithLocalFalse`] for that pair.
pub fn require_wildcard_locality(local: Option<bool>, entity_type: Option<&TypeExprRaw>, selectors: &[EntitySelectorParams]) -> Result<(), ParameterError> {
    match local {
        Some(false) if selects_every_type(entity_type, selectors) => Err(ParameterError::WildcardTypeWithLocalFalse),
        Some(false | true) | None => Ok(()),
    }
}

/// Whether the flat `type` or the `type` of any clause 5.2.33 selector is the
/// clause 4.17 `*` that selects every Entity Type.
fn selects_every_type(entity_type: Option<&TypeExprRaw>, selectors: &[EntitySelectorParams]) -> bool {
    entity_type
        .into_iter()
        .chain(selectors.iter().filter_map(|selector| selector.type_.as_ref()))
        .any(|selection| matches!(selection, TypeExprRaw::Wildcard))
}

/// Which of the clause's inputs one operation enumerates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdmittedNarrowings {
    /// Whether the clause lists the local scope as input (e).
    pub local_scope: LocalScopeNarrowing,
    /// Whether the clause counts a system Attribute in inputs (b) and (c).
    pub system_attributes: SystemAttributeNarrowing,
}

impl AdmittedNarrowings {
    /// The entity-facing rule of clauses 5.6.21.4, 5.7.2.4, 5.7.4.4 and 5.14.4.4.
    pub const ENTITIES: AdmittedNarrowings = AdmittedNarrowings {
        local_scope: LocalScopeNarrowing::Listed,
        system_attributes: SystemAttributeNarrowing::Discounted,
    };
    /// The registration-discovery rule of clause 5.10.2.4.
    pub const REGISTRATIONS: AdmittedNarrowings = AdmittedNarrowings {
        local_scope: LocalScopeNarrowing::Unlisted,
        system_attributes: SystemAttributeNarrowing::Counted,
    };
}

impl NarrowingInputs<'_> {
    /// Whether the request carries any input its operation counts as a narrowing.
    fn narrows(&self) -> bool {
        self.entity_type.is_some()
            || self.entity_selectors.iter().any(|selector| selector.type_.is_some())
            || self.geo.is_some()
            || self.attrs.is_some_and(|attrs| self.counts_any(attrs.iter()))
            || self.purged.is_some_and(|purged| self.counts_any(purged.iter()))
            || self.q.is_some_and(|q| self.counts_any(referenced_attributes(q).into_iter()))
            || matches!(self.admitted.local_scope, LocalScopeNarrowing::Listed) && self.local_scope == ExecutionScope::Local
    }

    /// Whether any of the named members narrows under this operation's rule.
    fn counts_any<'name>(&self, names: impl Iterator<Item = &'name AttributeName>) -> bool {
        let mut attributes = names.filter(|name| names_an_attribute(name));
        match self.admitted.system_attributes {
            SystemAttributeNarrowing::Counted => attributes.next().is_some(),
            SystemAttributeNarrowing::Discounted => attributes.any(|name| !is_system_attribute(name)),
        }
    }
}

/// Whether a name addresses an Attribute at all.
///
/// Inputs (b) and (c) are lists of *Attribute* names. `id` and `type` are
/// structural in every representation and name no Attribute; `scope` is "a
/// special scope Property" (clause 4.18), so it does.
fn names_an_attribute(name: &AttributeName) -> bool {
    !matches!(name.as_str(), "id" | "type")
}

/// Whether a name addresses one of the system Attributes.
///
/// The system Attributes are the temporal properties of clause 4.8, which is
/// also the set a temporal query selects an axis from.
fn is_system_attribute(name: &AttributeName) -> bool {
    TimeProp::iter().any(|property| name.as_str() == <&'static str>::from(property))
}

/// Require at least one of the narrowing inputs the operation's clause lists.
///
/// # Errors
///
/// Returns [`ParameterError::SelectorRequired`] naming the inputs the operation
/// admits when the request carries none of them -- the "too wide query" of
/// clauses 5.6.21.4, 5.7.2.4, 5.7.4.4, 5.10.2.4 and 5.14.4.4.
pub fn require_narrowing(inputs: &NarrowingInputs<'_>) -> Result<(), ParameterError> {
    if inputs.narrows() {
        Ok(())
    } else {
        Err(ParameterError::SelectorRequired(inputs.admitted.local_scope))
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        error::ParameterError,
        query::deserialization::OptionalGeoQuery,
        query_narrowing::{AdmittedNarrowings, ExecutionScope, NarrowingInputs, require_narrowing},
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::{filter::expression::Expr, geo::input::GeoQueryInput, type_selection::grammar::TypeExprRaw};

    fn attrs(names: &[&str]) -> Vec<AttributeName> {
        names.iter().map(|name| name.parse().expect("a valid Attribute name")).collect()
    }

    fn entity_type() -> TypeExprRaw {
        "Vehicle".parse().expect("a valid type selector")
    }

    fn filter(q: &str) -> Expr {
        q.parse().expect("a valid q")
    }

    fn geo() -> GeoQueryInput {
        serde_urlencoded::from_str::<OptionalGeoQuery>("georel=near%3BmaxDistance==100&geometry=Point&coordinates=[1,2]")
            .expect("a valid geoquery")
            .0
            .expect("present")
    }

    /// Inputs (a) to (d) each narrow an entity query on their own.
    #[test]
    fn each_content_narrowing_satisfies_the_rule_on_its_own() {
        let selector = entity_type();
        let names = attrs(&["speed"]);
        let q = filter("speed>10");
        let reference = geo();
        for inputs in [
            NarrowingInputs::builder()
                .admitted(AdmittedNarrowings::ENTITIES)
                .entity_type(Some(&selector))
                .build(),
            NarrowingInputs::builder().admitted(AdmittedNarrowings::ENTITIES).attrs(Some(&names)).build(),
            NarrowingInputs::builder().admitted(AdmittedNarrowings::ENTITIES).q(Some(&q)).build(),
            NarrowingInputs::builder().admitted(AdmittedNarrowings::ENTITIES).geo(Some(&reference)).build(),
        ] {
            assert!(require_narrowing(&inputs).is_ok());
        }
    }

    /// Input (e): the local scope narrows an entity query and is not among the
    /// inputs clause 5.10.2.4 lists for a registration discovery.
    #[test]
    fn the_local_scope_narrows_only_where_the_clause_lists_it() {
        let local = NarrowingInputs::builder()
            .admitted(AdmittedNarrowings::ENTITIES)
            .local_scope(ExecutionScope::Local)
            .build();
        assert!(require_narrowing(&local).is_ok());
        let discovery = NarrowingInputs::builder()
            .admitted(AdmittedNarrowings::REGISTRATIONS)
            .local_scope(ExecutionScope::Local)
            .build();
        assert!(matches!(require_narrowing(&discovery), Err(ParameterError::SelectorRequired(_))));
    }

    /// A request carrying none of the inputs is refused.
    #[test]
    fn a_request_with_no_narrowing_input_is_too_wide() {
        let bare = NarrowingInputs::builder().admitted(AdmittedNarrowings::ENTITIES).build();
        let refused = require_narrowing(&bare).expect_err("too wide");
        assert!(refused.to_string().contains("too wide query"), "{refused}");
    }

    /// Inputs (b) and (c) need "at least one non-system Attribute" where the
    /// entity-facing clauses say so, and take any Attribute where clause
    /// 5.10.2.4 does not.
    #[test]
    fn a_system_attribute_narrows_only_a_registration_discovery() {
        let system = attrs(&["createdAt", "modifiedAt", "deletedAt", "observedAt"]);
        let q = filter("createdAt>2020-01-01T00:00:00Z");
        let entities = NarrowingInputs::builder()
            .admitted(AdmittedNarrowings::ENTITIES)
            .attrs(Some(&system))
            .q(Some(&q))
            .build();
        assert!(matches!(require_narrowing(&entities), Err(ParameterError::SelectorRequired(_))));
        let discovery = NarrowingInputs::builder()
            .admitted(AdmittedNarrowings::REGISTRATIONS)
            .attrs(Some(&system))
            .build();
        assert!(require_narrowing(&discovery).is_ok());
    }

    /// One non-system Attribute beside the system ones is enough.
    #[test]
    fn one_non_system_attribute_in_the_list_narrows() {
        let mixed = attrs(&["createdAt", "speed"]);
        let inputs = NarrowingInputs::builder().admitted(AdmittedNarrowings::ENTITIES).attrs(Some(&mixed)).build();
        assert!(require_narrowing(&inputs).is_ok());
    }

    /// An empty `attrs` names no Attribute at all and narrows nothing.
    #[test]
    fn an_empty_attribute_list_narrows_nothing() {
        let empty = attrs(&[]);
        let inputs = NarrowingInputs::builder().admitted(AdmittedNarrowings::ENTITIES).attrs(Some(&empty)).build();
        assert!(matches!(require_narrowing(&inputs), Err(ParameterError::SelectorRequired(_))));
    }
}

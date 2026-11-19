//! A `q` filter's names looked up in the tenant's dictionary, and the filter
//! folded by the terms that name something it does not hold.
//!
//! A row and each element of its `sub_attributes` name their attribute by
//! dictionary key, a hop reads its Relationship's instances by key, and a
//! type guard reads a linked entity's types by key, so a name the dictionary
//! never recorded names nothing a stored instance holds. A term naming one
//! holds for no Entity under either polarity, since a complement too asks for
//! a target the Entity holds (clause 4.9), and a guard type never recorded is
//! carried by no entity. A conjunction holding such a term holds for no
//! Entity, a disjunction holds as its other children do, and a guard admits
//! the types it names that were recorded. The fold is taken once, here, so no
//! rendering reads a name.

use crate::query_names::QueryNames;
use andromeda_ngsi_ld_query::filter::reading::{filter_reading::FilterReading, hop::Hop, term_reading::TermReading};
use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey};

/// The boolean structure of a filter whose every term is resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedFilter<'r> {
    /// Every child holds.
    All(Vec<ResolvedFilter<'r>>),
    /// Some child holds; none is absent.
    Any(Vec<ResolvedFilter<'r>>),
    /// One Query Term.
    Term(ResolvedTerm<'r>),
}

/// A term reading and the dictionary keys of every name it reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTerm<'r> {
    /// The term.
    term: &'r TermReading,
    /// Each hop, outermost first.
    hops: Vec<ResolvedHop>,
    /// The key of the attribute the last hop reaches.
    attribute: AttributeNameKey,
    /// The key of each sub-attribute below the attribute, outermost first.
    chain: Vec<AttributeNameKey>,
}

/// One hop's Relationship and type guard, by key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedHop {
    /// The key of the Relationship the hop crosses.
    relationship: AttributeNameKey,
    /// The entities the hop admits.
    guard: HopGuard,
}

/// The entities a hop admits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HopGuard {
    /// Any entity: the term names no guard.
    AnyEntity,
    /// An entity carrying one of the types, never empty: the guard types the
    /// dictionary recorded.
    OneOf(Vec<EntityTypeNameKey>),
}

impl<'r> ResolvedFilter<'r> {
    /// `reading` with its names resolved through `names`, `None` when it
    /// holds for no Entity: a term names something the dictionary does not
    /// hold, and the fold carries that through every group above it.
    #[must_use]
    pub fn of(reading: &'r FilterReading, names: &QueryNames) -> Option<ResolvedFilter<'r>> {
        match reading {
            FilterReading::All(children) => children
                .iter()
                .map(|child| ResolvedFilter::of(child, names))
                .collect::<Option<Vec<_>>>()
                .map(ResolvedFilter::All),
            FilterReading::Any(children) => {
                let kept: Vec<ResolvedFilter<'r>> = children.iter().filter_map(|child| ResolvedFilter::of(child, names)).collect();
                (!kept.is_empty()).then_some(ResolvedFilter::Any(kept))
            }
            FilterReading::Term(term) => ResolvedTerm::of(term, names).map(ResolvedFilter::Term),
        }
    }
}

impl<'r> ResolvedTerm<'r> {
    /// `term` with its names resolved through `names`, `None` when one of them
    /// was never recorded or a guard admits no recorded type.
    #[must_use]
    pub fn of(term: &'r TermReading, names: &QueryNames) -> Option<ResolvedTerm<'r>> {
        let hops = term.hops().iter().map(|hop| ResolvedHop::of(hop, names)).collect::<Option<Vec<_>>>()?;
        let attribute = names.attributes.get_str(term.attribute().as_str())?;
        let chain = term
            .sub_attributes()
            .iter()
            .map(|name| names.attributes.get(name))
            .collect::<Option<Vec<_>>>()?;
        Some(ResolvedTerm { term, hops, attribute, chain })
    }

    /// The term.
    #[must_use]
    pub const fn term(&self) -> &'r TermReading {
        self.term
    }

    /// Each hop, outermost first.
    #[must_use]
    pub const fn hops(&self) -> &[ResolvedHop] {
        self.hops.as_slice()
    }

    /// The key of the attribute the last hop reaches.
    #[must_use]
    pub const fn attribute(&self) -> AttributeNameKey {
        self.attribute
    }

    /// The key of each sub-attribute below the attribute, outermost first.
    #[must_use]
    pub const fn chain(&self) -> &[AttributeNameKey] {
        self.chain.as_slice()
    }

    /// The key of the attribute of the Entity under evaluation the term reads
    /// first: the first hop's Relationship, else the attribute. An Entity the
    /// term holds for holds an instance of it, since every term is existential
    /// over the instances of what it reads (clause 4.9).
    #[must_use]
    pub fn carrier(&self) -> AttributeNameKey {
        self.hops.first().map_or(self.attribute, |hop| hop.relationship)
    }
}

impl ResolvedHop {
    /// `hop` with its Relationship and guard types resolved, `None` when the
    /// Relationship was never recorded or the guard names no recorded type.
    fn of(hop: &Hop, names: &QueryNames) -> Option<ResolvedHop> {
        let relationship = names.attributes.get_str(hop.relationship().as_str())?;
        let guard = match hop.guard_types() {
            [] => HopGuard::AnyEntity,
            types => {
                let recorded: Vec<EntityTypeNameKey> = types.iter().filter_map(|guard_type| names.types.get_str(guard_type.as_str())).collect();
                if recorded.is_empty() {
                    return None;
                }
                HopGuard::OneOf(recorded)
            }
        };
        Some(ResolvedHop { relationship, guard })
    }

    /// The key of the Relationship the hop crosses.
    #[must_use]
    pub const fn relationship(&self) -> AttributeNameKey {
        self.relationship
    }

    /// The entities the hop admits.
    #[must_use]
    pub const fn guard(&self) -> &HopGuard {
        &self.guard
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        q_predicate::{
            lowered_filter::reading_of,
            resolved_filter::{HopGuard, ResolvedFilter, ResolvedHop},
        },
        query_names::QueryNames,
    };
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};
    use pretty_assertions::assert_eq;

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("a", 1), ("b", 2), ("rel", 3), ("s", 4)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::from_pairs([("https://example.com/T".parse().expect("valid type name"), EntityTypeNameKey::from(7))]),
            ..QueryNames::default()
        }
    }

    /// The attribute keys of every term `q` resolves to, `None` when it holds
    /// for no Entity.
    fn resolved_attributes(q: &str) -> Option<Vec<i32>> {
        let reading = reading_of(q);
        let resolved = ResolvedFilter::of(&reading, &names())?;
        let mut keys = Vec::new();
        collect(&resolved, &mut keys);
        Some(keys)
    }

    fn collect(filter: &ResolvedFilter<'_>, keys: &mut Vec<i32>) {
        match filter {
            ResolvedFilter::All(children) | ResolvedFilter::Any(children) => children.iter().for_each(|child| collect(child, keys)),
            ResolvedFilter::Term(term) => keys.push(term.attribute().get()),
        }
    }

    /// A name never recorded names nothing a stored instance holds, so a
    /// conjunction holding its term holds for no Entity.
    #[test]
    fn a_conjunction_holding_an_unrecorded_name_holds_for_no_entity() {
        assert_eq!(resolved_attributes("\"https://example.com/a\"==1;\"https://example.com/z\"==2"), None);
        assert_eq!(resolved_attributes("\"https://example.com/a\"==1;\"https://example.com/z\"!=2"), None);
        assert_eq!(resolved_attributes("\"https://example.com/a\".\"https://example.com/z\""), None);
        assert_eq!(resolved_attributes("\"https://example.com/z\"{\"https://example.com/a\"}==1"), None);
    }

    /// A disjunction holds as the arms that name recorded names do.
    #[test]
    fn a_disjunction_drops_the_arm_naming_an_unrecorded_name() {
        assert_eq!(
            resolved_attributes("\"https://example.com/a\"==1|\"https://example.com/z\"==2|\"https://example.com/b\"==3"),
            Some(vec![1, 2])
        );
        assert_eq!(resolved_attributes("\"https://example.com/y\"==1|\"https://example.com/z\"==2"), None);
        assert_eq!(
            resolved_attributes("\"https://example.com/a\"==1;(\"https://example.com/z\"==2|\"https://example.com/b\"==3)"),
            Some(vec![1, 2])
        );
    }

    /// A guard type never recorded is carried by no entity, so a guard admits
    /// the recorded types alone, and a hop whose guard admits none reaches no
    /// entity.
    #[test]
    fn a_guard_admits_its_recorded_types_alone() {
        let reading = reading_of("\"https://example.com/rel\"{\"https://example.com/T\",\"https://example.com/U\":\"https://example.com/a\"}==1");
        let Some(ResolvedFilter::Term(term)) = ResolvedFilter::of(&reading, &names()) else {
            panic!("one resolved term");
        };
        assert_eq!(
            term.hops().first().map(ResolvedHop::guard),
            Some(&HopGuard::OneOf(vec![EntityTypeNameKey::from(7)]))
        );
        assert_eq!(term.hops().first().map(|hop| hop.relationship().get()), Some(3));
        assert_eq!(
            resolved_attributes("\"https://example.com/rel\"{\"https://example.com/U\":\"https://example.com/a\"}==1"),
            None
        );
        let unguarded = reading_of("\"https://example.com/rel\"{\"https://example.com/a\"}==1");
        let Some(ResolvedFilter::Term(term)) = ResolvedFilter::of(&unguarded, &names()) else {
            panic!("one resolved term");
        };
        assert_eq!(term.hops().first().map(ResolvedHop::guard), Some(&HopGuard::AnyEntity));
    }

    /// A sub-attribute chain resolves to its keys, outermost first.
    #[test]
    fn a_sub_attribute_chain_resolves_outermost_first() {
        let reading = reading_of("\"https://example.com/a\".\"https://example.com/s\".\"https://example.com/b\"==1");
        let Some(ResolvedFilter::Term(term)) = ResolvedFilter::of(&reading, &names()) else {
            panic!("one resolved term");
        };
        assert_eq!(term.chain().iter().map(|key| key.get()).collect::<Vec<_>>(), [4, 2]);
    }
}

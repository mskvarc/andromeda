//! Term compaction of an expanded `q` filter (clause 4.9, clause 5.5.7).
//!
//! The inverse of [`crate::filter::expansion`]: a stored filter carries the
//! expanded IRIs it was written with, and a reader receives it spelled in the
//! terms of the `@context` the read supplied. Every position expansion
//! rewrote (the attribute, its sub-attributes, the compound-value members, a
//! hop's Relationship and its entity-type guard, and the operands of a path
//! ending on `valueType` or `objectType`) is rewritten back here, and nothing
//! else is touched: operators, literal operands and the boolean structure come
//! back exactly as stored.
//!
//! As with expansion, the walker lives beside the AST it rewrites and asks a
//! [`TermCompactor`] for the one thing it needs from JSON-LD.

use crate::{
    filter::{
        Filter,
        attribute_path::AttributePath,
        filter_error::FilterError,
        json_key_attributes::JsonKeyAttributes,
        nested_names::NestedNames,
        operand_coercion::OperandCoercion,
        query_term::QueryTerm,
        trailing_group::TrailingGroup,
        value_path::ValuePath,
    },
    name_stage::{Expanded, NameStage, Written},
    term_compactor::TermCompactor,
};
use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, attribute_name::AttributeName};
use std::{collections::HashSet, convert::Infallible, future::Future, hash::BuildHasher, pin::Pin};

/// Recursively compact every attribute term of a `q` filter.
///
/// The boolean structure, the operators and the literal operands survive
/// unchanged, and the operands a path ending on `valueType` or `objectType`
/// expanded compact by the inverse rule (see [`OperandCoercion::Vocabulary`]),
/// so `expand_filter` applied to the result under the same `@context` yields
/// the filter this was given.
///
/// # Errors
///
/// Returns [`FilterError::InvalidAttributeName`] when a hop's guard type has no
/// written name: no term abbreviates it and it is longer than a name may be.
pub async fn compact_filter(filter: Filter<Expanded>, compactor: &impl TermCompactor) -> Result<Filter<Written>, FilterError> {
    let json_keys = JsonKeyAttributes::default();
    let members = NestedNames::Terms(&json_keys);
    map_terms(filter, &|term| compact_query_term(term, members, compactor)).await
}

/// Compact the operands of one `q` query term whose path ends on a member
/// holding a vocabulary term, then its attribute path.
///
/// The operands go first: the member is recognised by its expanded name.
async fn compact_query_term(term: QueryTerm<Expanded>, members: NestedNames<'_>, compactor: &impl TermCompactor) -> Result<QueryTerm<Written>, FilterError> {
    let term = match term.path().terminal().ending_member() {
        Some((member, _)) if member.holds_vocabulary_term() => OperandCoercion::Vocabulary.compact_operands(term, compactor).await,
        Some(_) | None => term,
    };
    compact_query_term_paths(term, members, compactor).await
}

/// Recursively compact every property term of a `csf` filter, leaving its
/// compound-value members as written.
///
/// The inverse of [`crate::filter::expansion::expand_context_source_filter`]: the members
/// address raw JSON keys (see [`NestedNames::Verbatim`]), which no
/// `@context` abbreviates.
///
/// # Errors
///
/// As [`compact_filter`].
pub async fn compact_context_source_filter(filter: Filter<Expanded>, compactor: &impl TermCompactor) -> Result<Filter<Written>, FilterError> {
    map_terms(filter, &|term| compact_query_term_paths(term, NestedNames::Verbatim, compactor)).await
}

/// Rewrite every query term of a filter with `handler`, leaving its
/// boolean structure intact.
async fn map_terms<S, T, E, F, Fut>(filter: Filter<S>, handler: &F) -> Result<Filter<T>, E>
where
    S: NameStage,
    T: NameStage,
    F: Fn(QueryTerm<S>) -> Fut + Sync,
    Fut: Future<Output = Result<QueryTerm<T>, E>> + Send,
    E: Send,
{
    match filter {
        Filter::And(filters) => Ok(Filter::And(map_all_terms(filters, handler).await?)),
        Filter::Or(filters) => Ok(Filter::Or(map_all_terms(filters, handler).await?)),
        Filter::Term(term) => Ok(Filter::Term(handler(term).await?)),
    }
}

/// Rewrite the operands of one boolean node in order.
async fn map_all_terms<S, T, E, F, Fut>(filters: Vec<Filter<S>>, handler: &F) -> Result<Vec<Filter<T>>, E>
where
    S: NameStage,
    T: NameStage,
    F: Fn(QueryTerm<S>) -> Fut + Sync,
    Fut: Future<Output = Result<QueryTerm<T>, E>> + Send,
    E: Send,
{
    let mut rewritten = Vec::with_capacity(filters.len());
    for filter in filters {
        rewritten.push(map_terms_boxed(filter, handler).await?);
    }
    Ok(rewritten)
}

/// Box-pinned wrapper: the walker recurses, and an `async fn` cannot name its
/// own future type.
fn map_terms_boxed<'a, S, T, E, F, Fut>(filter: Filter<S>, handler: &'a F) -> Pin<Box<dyn Future<Output = Result<Filter<T>, E>> + Send + 'a>>
where
    S: NameStage,
    T: NameStage,
    F: Fn(QueryTerm<S>) -> Fut + Sync,
    Fut: Future<Output = Result<QueryTerm<T>, E>> + Send + 'a,
    E: Send + 'a,
{
    Box::pin(map_terms(filter, handler))
}

/// Undo the `expandValues` coercion of clause 5.2.12 on a stored filter.
///
/// Table 5.2.12-1 defines `expandValues` as a query-execution instruction:
/// the values of the named attributes "should be expanded against the supplied
/// @context using JSON-LD type coercion **prior to executing the query**", so
/// the coercion belongs to the filter a match runs, not to the filter the
/// client wrote. A Subscription echoes the one the client wrote, and this is
/// what puts the operands back the way they arrived: the exact inverse of
/// [`crate::filter::expansion::apply_expand_values`], over exactly the
/// attributes that opted in.
///
/// `expand_attributes` is the stored, expanded opt-in list, and the paths it is
/// matched against are the stored, expanded ones, so this runs before
/// [`compact_filter`] turns those paths back into terms.
pub async fn revert_expand_values<H: BuildHasher + Sync>(
    filter: Filter<Expanded>,
    expand_attributes: &HashSet<AttributeIri, H>,
    compactor: &impl TermCompactor,
) -> Filter<Expanded> {
    let reverted =
        map_terms::<Expanded, Expanded, Infallible, _, _>(
            filter,
            &|term| async move { Ok(revert_equality_values(term, expand_attributes, compactor).await) },
        )
        .await;
    let Ok(filter) = reverted;
    filter
}

/// Put the operands of a comparison back in the client's terms when its
/// attribute opted in via `expandValues`.
async fn revert_equality_values<H: BuildHasher + Sync>(
    term: QueryTerm<Expanded>,
    expand_attributes: &HashSet<AttributeIri, H>,
    compactor: &impl TermCompactor,
) -> QueryTerm<Expanded> {
    if expand_attributes.contains(term.path().terminal().attribute.as_str()) {
        return OperandCoercion::ExpandValues.compact_operands(term, compactor).await;
    }
    term
}

/// Compact the attribute path of a single query term, keeping its operator and
/// operand.
async fn compact_query_term_paths(
    term: QueryTerm<Expanded>,
    members: NestedNames<'_>,
    compactor: &impl TermCompactor,
) -> Result<QueryTerm<Written>, FilterError> {
    Ok(match term {
        QueryTerm::Exists(path) => QueryTerm::Exists(compact_attribute_path(path, members, compactor).await?),
        QueryTerm::Equality { path, membership, value } => QueryTerm::Equality {
            path: compact_attribute_path(path, members, compactor).await?,
            membership,
            value,
        },
        QueryTerm::Ordering { path, operator, value } => QueryTerm::Ordering {
            path: compact_attribute_path(path, members, compactor).await?,
            operator,
            value,
        },
        QueryTerm::MatchPattern { path, pattern } => QueryTerm::MatchPattern {
            path: compact_attribute_path(path, members, compactor).await?,
            pattern,
        },
        QueryTerm::NotMatchPattern { path, pattern } => QueryTerm::NotMatchPattern {
            path: compact_attribute_path(path, members, compactor).await?,
            pattern,
        },
    })
}

/// Compact an attribute path: every segment of its value path as a term, and for
/// each linked-entity hop the Relationship as a term and its guard as entity
/// types.
/// This is the same split [`crate::filter::expansion::expand_attribute_path`] makes
/// in the other direction.
///
/// # Errors
///
/// As [`compact_filter`].
pub async fn compact_attribute_path(
    path: AttributePath<Expanded>,
    members: NestedNames<'_>,
    compactor: &impl TermCompactor,
) -> Result<AttributePath<Written>, FilterError> {
    match path {
        AttributePath::Local(local) => Ok(AttributePath::Local(compact_value_path(local, members, compactor).await)),
        AttributePath::Linked {
            relationship,
            guard_types,
            inner,
        } => {
            let relationship = compactor.compact_term(&relationship).await;
            let mut compacted_guard = Vec::with_capacity(guard_types.len());
            for guard_type in &guard_types {
                compacted_guard.push(compactor.compact_type(guard_type).await.map_err(FilterError::InvalidAttributeName)?);
            }
            let inner = compact_attribute_path_boxed(*inner, members, compactor).await?;
            Ok(AttributePath::Linked {
                relationship,
                guard_types: compacted_guard,
                inner: Box::new(inner),
            })
        }
    }
}

/// Box-pinned wrapper: the attribute path walker recurses through hops, and an
/// `async fn` cannot name its own future type.
fn compact_attribute_path_boxed<'a, C: TermCompactor>(
    path: AttributePath<Expanded>,
    members: NestedNames<'a>,
    compactor: &'a C,
) -> Pin<Box<dyn Future<Output = Result<AttributePath<Written>, FilterError>> + Send + 'a>> {
    Box::pin(compact_attribute_path(path, members, compactor))
}

/// Compact a value path (attribute, sub-attributes and, when
/// `members` makes them terms, compound-value member names) to terms.
///
/// The member names become the path's written spelling as well: a freshly
/// parsed path carries what the client wrote in both spellings of its
/// [`MemberPath`](crate::filter::trailing_group::MemberPath), and a compacted path is meant to read as one. A target
/// language is no term and stays as written.
pub async fn compact_value_path(path: ValuePath<Expanded>, members: NestedNames<'_>, compactor: &impl TermCompactor) -> ValuePath<Written> {
    let trailing = match path.trailing {
        Some(TrailingGroup::Members(mut member_path)) => {
            if members.are_terms_under(&path.attribute) {
                member_path.addressed = compact_all(&member_path.addressed, compactor).await;
            }
            member_path.written.clone_from(&member_path.addressed); // clone: the written and addressed spellings are separate owned values
            Some(TrailingGroup::Members(member_path))
        }
        other @ (Some(TrailingGroup::Language(_) | TrailingGroup::AllLanguages) | None) => other,
    };
    let sub_attributes = if members.sub_attributes_are_terms() {
        compact_all(&path.sub_attributes, compactor).await
    } else {
        path.sub_attributes
    };
    ValuePath {
        attribute: compactor.compact_term(&path.attribute).await,
        sub_attributes,
        trailing,
    }
}

/// Compact every name in a list.
///
/// A name that is no absolute IRI is a literal key of a value, which no term
/// abbreviates, and stays as it is.
async fn compact_all(names: &[AttributeName], compactor: &impl TermCompactor) -> Vec<AttributeName> {
    let mut compacted = Vec::with_capacity(names.len());
    for name in names {
        compacted.push(match name.as_str().parse::<AttributeIri>() {
            Ok(iri) => compactor.compact_term(&iri).await,
            Err(_literal_key) => name.clone(),
        });
    }
    compacted
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::{
            Filter,
            compaction::{compact_context_source_filter, compact_filter, revert_expand_values},
            expansion::{apply_expand_values, expand_context_source_filter, expand_filter},
            json_key_attributes::JsonKeyAttributes,
        },
        name_stage::{Expanded, Written},
        namespace_expander::{EXAMPLE_NAMESPACE_TYPED, NamespaceExpander},
        term_compactor::TermCompactor,
    };
    use andromeda_ngsi_ld_lexicon::{
        attribute_iri::AttributeIri,
        attribute_name::AttributeName,
        entity_type_iri::EntityTypeIri,
        entity_type_name::EntityTypeName,
        reference::NgsiLdReferenceError,
    };
    use andromeda_ngsi_ld_vocabulary::expanded::properties::{OBJECT_TYPE, VALUE_TYPE};
    use iri_rs::Iri;
    use std::{
        collections::HashSet,
        future::{Future, ready},
    };

    const TERM_PREFIX: &str = "https://example.com/";
    const TYPE_PREFIX: &str = "https://example.com/types/";

    /// The core members a path ends on in [`SHAPES`], each with the term the
    /// pair below expands to its core IRI and compacts back to.
    const CORE_MEMBERS: &[(&str, Iri<&str>)] = &[("valueType", VALUE_TYPE), ("objectType", OBJECT_TYPE)];

    /// The expander paired below with the compactor that strips the same
    /// prefixes, so the pair is an exact inverse.
    const EXPANDER: NamespaceExpander = EXAMPLE_NAMESPACE_TYPED.with_core_members(CORE_MEMBERS);

    /// The inverse of [`EXPANDER`]: a name carrying the prefix loses it,
    /// any other name is answered as given, which is the clause 4.5.1 rule an
    /// implementation follows for an IRI no term abbreviates.
    struct PrefixCompactor;

    fn strip<'a>(name: &'a str, prefix: &str) -> &'a str {
        let core = CORE_MEMBERS.iter().find(|(_, iri)| iri.as_str() == name).map(|(term, _)| *term);
        core.or_else(|| name.strip_prefix(prefix)).unwrap_or(name)
    }

    impl TermCompactor for PrefixCompactor {
        fn compact_term(&self, name: &AttributeIri) -> impl Future<Output = AttributeName> + Send {
            ready(strip(name.as_str(), TERM_PREFIX).parse().expect("a stripped term is a valid attribute name"))
        }

        fn compact_type(&self, name: &EntityTypeIri) -> impl Future<Output = Result<EntityTypeName, NgsiLdReferenceError>> + Send {
            ready(strip(name.as_str(), TYPE_PREFIX).parse())
        }
    }

    /// The filter `text` writes, whose names are absolute IRIs already.
    fn stored(text: &str) -> Filter<Expanded> {
        text.parse::<Filter<Written>>().expect("parses").into_expanded().expect("absolute names")
    }

    /// The suite's operators and path shapes: every operand kind, every
    /// operator, the three local axes, a guarded and an unguarded hop, and the
    /// boolean structure with an explicit group.
    const SHAPES: [&str; 20] = [
        "speed>50",
        "speed>=50",
        "speed<50",
        "speed<=50",
        "speed==10..20",
        "speed!=10,20,30",
        "active==true",
        "colour==\"red\",\"blue\"",
        "observed>=2017-12-24T12:00:00Z",
        "moment==\"12:00:00Z\"..\"13:00:00Z\"",
        "name~=^A.*",
        "name!~=^A.*",
        "speed",
        "temperature.observedAt<2024-01-01T00:00:00Z",
        "address[city]==\"Berlin\"",
        "sensor.rawdata[airquality.particulate]==40",
        "isParkedIn{Building,Store:category}==\"x\"",
        "(a|b);c{d{e}}==5",
        "reading.valueType==\"String\"",
        "reading.objectType!=\"Device\",\"Sensor\"",
    ];

    /// Clause 5.5.7 in both directions: a filter expanded when the resource
    /// was written and compacted when it is read must come back as the filter
    /// the client wrote, for every operator and path shape the grammar has.
    #[tokio::test]
    async fn compacting_an_expanded_filter_restores_what_the_client_wrote() {
        for shape in SHAPES {
            let parsed = shape.parse::<Filter<Written>>().unwrap_or_else(|error| panic!("{shape} must parse: {error}"));
            let expanded = expand_filter(parsed.clone(), &JsonKeyAttributes::default(), &EXPANDER).await.expect("expands");
            assert_ne!(expanded.to_string(), shape, "the expansion of {shape} must differ from it");

            let compacted = compact_filter(expanded, &PrefixCompactor).await.expect("compacts");

            pretty_assertions::assert_eq!(compacted, parsed, "compaction of the expansion of {shape}");
            pretty_assertions::assert_eq!(compacted.to_string(), shape, "serialisation after compaction of {shape}");
        }
    }

    /// The other composition order: expanding a compacted filter yields the
    /// expanded filter again, so a stored filter survives a read-back and a
    /// re-write unchanged.
    #[tokio::test]
    async fn expanding_a_compacted_filter_restores_the_stored_expansion() {
        for shape in SHAPES {
            let parsed = shape.parse::<Filter<Written>>().unwrap_or_else(|error| panic!("{shape} must parse: {error}"));
            let expanded = expand_filter(parsed, &JsonKeyAttributes::default(), &EXPANDER).await.expect("expands");

            let compacted = compact_filter(expanded.clone(), &PrefixCompactor).await.expect("compacts");
            let re_expanded = expand_filter(compacted, &JsonKeyAttributes::default(), &EXPANDER).await.expect("expands again");

            pretty_assertions::assert_eq!(re_expanded, expanded, "expand after compact of {shape}");
        }
    }

    /// An IRI the context does not abbreviate stays a full IRI (clause 4.5.1),
    /// and the operator and operand beside it are untouched.
    #[tokio::test]
    async fn a_name_no_term_abbreviates_is_kept_as_the_full_iri() {
        let expanded = stored("\"https://other.example/speed\">50;\"https://example.com/colour\"==\"red\"");

        let compacted = compact_filter(expanded, &PrefixCompactor).await.expect("compacts");

        pretty_assertions::assert_eq!(compacted.to_string(), "\"https://other.example/speed\">50;colour==\"red\"");
    }

    /// A hop's two halves compact through different vocabularies, the mirror
    /// of expansion: the Relationship is an attribute term, the guard names
    /// entity types.
    #[tokio::test]
    async fn a_hop_compacts_its_relationship_as_a_term_and_its_guard_as_entity_types() {
        let expanded = stored(
            "\"https://example.com/isParkedIn\"{\"https://example.com/types/Building\",\"https://example.com/types/Store\":\"https://example.com/category\"}==\"x\"",
        );

        let compacted = compact_filter(expanded, &PrefixCompactor).await.expect("compacts");

        pretty_assertions::assert_eq!(compacted.to_string(), "isParkedIn{Building,Store:category}==\"x\"");
    }

    /// A `csf` round-trips with its sub-properties and trailing members
    /// exactly as written, even a name that reads like a term the compactor
    /// would abbreviate: those are the raw JSON keys of a Context Source
    /// Property (clause 4.9).
    #[tokio::test]
    async fn a_csf_round_trips_with_its_sub_properties_and_trailing_members_as_written() {
        let written = "coverage.\"https://example.com/since\"[\"https://example.com/area\".region]==\"north\"";
        let parsed = written.parse::<Filter<Written>>().expect("parses");
        let expanded = expand_context_source_filter(parsed.clone(), &EXPANDER).await.expect("expands");

        let compacted = compact_context_source_filter(expanded.clone(), &PrefixCompactor).await.expect("compacts");

        pretty_assertions::assert_eq!(compacted, parsed);
        pretty_assertions::assert_ne!(
            compact_filter(expanded, &PrefixCompactor).await.expect("compacts"),
            parsed,
            "an Entity filter compacts the same member as a term"
        );
    }

    /// A quoted word `expandValues` expanded reads back quoted, as the client
    /// wrote it, so a stored filter is answered in the form it was written in.
    #[tokio::test]
    async fn an_expanded_quoted_word_reverts_to_the_quoted_word() {
        let written = "gender==\"Male\"";
        let parsed = written.parse::<Filter<Written>>().expect("parses");
        let expanded = expand_filter(parsed, &JsonKeyAttributes::default(), &EXPANDER).await.expect("expands");
        let opted_in: HashSet<AttributeIri> = HashSet::from([format!("{TERM_PREFIX}gender").parse().expect("a valid name")]);
        let coerced = apply_expand_values(expanded, &opted_in, &EXPANDER).await.expect("expands");

        let reverted = compact_filter(revert_expand_values(coerced, &opted_in, &PrefixCompactor).await, &PrefixCompactor)
            .await
            .expect("compacts");

        pretty_assertions::assert_eq!(reverted.to_string(), written);
    }
}

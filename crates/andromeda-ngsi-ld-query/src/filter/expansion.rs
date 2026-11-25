//! Term expansion of a parsed `q` filter (clause 4.9, clause 6.4.3.2).
//!
//! A filter arrives with compact attribute names and, for `expandValues` and
//! `jsonKeys`, compact values and object keys too. Each of those becomes a full
//! IRI here, before anything downstream tries to address a stored attribute
//! with it.
//!
//! These walkers live beside the AST they rewrite rather than in the JSON-LD
//! layer. They are shaped entirely by `Filter`, `QueryTerm`, `ValuePath` and
//! `Scalar`; the only thing they need from JSON-LD is a
//! [`TermExpander`].

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
    query_error::QueryError,
    term_expander::{TermExpander, TermExpansionError},
};
use andromeda_ngsi_ld_lexicon::{
    attribute_iri::AttributeIri,
    attribute_name::AttributeName,
    entity_type_iri::EntityTypeIri,
    identifier_length::IdentifierTooLong,
};
use std::{collections::HashSet, future::Future, hash::BuildHasher, pin::Pin};

/// Recursively expand attribute terms in a `q` filter.
///
/// Every attribute path in the filter (each value path's attribute and
/// sub-attributes, each hop's Relationship and guard types) is expanded to its
/// full IRI form, and so are the compound-value member names of every
/// attribute `json_keys` did not declare to hold raw JSON. The operands of a
/// query term whose path ends on `valueType` or `objectType` expand too (see
/// [`OperandCoercion::Vocabulary`]).
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`] when a term does not expand, carrying
/// [`TermExpansionError::NotAnIri`] when it expands to something that is not an
/// absolute IRI, and [`QueryError::Filter`] carrying
/// [`FilterError::InvalidAttributeName`] when a term does not expand to a usable
/// attribute name.
pub async fn expand_filter(filter: Filter<Written>, json_keys: &JsonKeyAttributes, expander: &impl TermExpander) -> Result<Filter<Expanded>, QueryError> {
    let members = NestedNames::Terms(json_keys);
    walk_terms(filter, &|term| expand_query_term(term, members, expander)).await
}

/// Expand the attribute path of one `q` query term, then the operands of one
/// whose path ends on a member holding a vocabulary term.
async fn expand_query_term(term: QueryTerm<Written>, members: NestedNames<'_>, expander: &impl TermExpander) -> Result<QueryTerm<Expanded>, QueryError> {
    let term = expand_query_term_paths(term, members, expander).await?;
    match term.path().terminal().ending_member() {
        Some((member, _)) if member.holds_vocabulary_term() => OperandCoercion::Vocabulary.expand_operands(term, expander).await,
        Some(_) | None => Ok(term),
    }
}

/// Recursively expand the property terms of a `csf` filter.
///
/// Every attribute path's property expands to a full IRI, as a registration's
/// Context Source Properties are held under their expanded names (clause
/// 5.5.7). Its sub-properties and compound-value members never do, because a
/// property's value is the raw JSON its registrant wrote, keyed as written
/// (see [`NestedNames::Verbatim`]).
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`] when a term does not expand, carrying
/// [`TermExpansionError::NotAnIri`] when it expands to something that is not an
/// absolute IRI, and [`QueryError::Filter`] carrying
/// [`FilterError::InvalidAttributeName`] when a term does not expand to a usable
/// attribute name.
pub async fn expand_context_source_filter(filter: Filter<Written>, expander: &impl TermExpander) -> Result<Filter<Expanded>, QueryError> {
    walk_terms(filter, &|term| expand_query_term_paths(term, NestedNames::Verbatim, expander)).await
}

/// Apply `expandValues`: for attributes in `expand_attributes`, expand string
/// operands through the `@context` (clause 6.4.3.2).
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`] when a string operand of an opted-in
/// attribute does not expand, carrying [`TermExpansionError::NotAnIri`] when it
/// expands to something that is not an absolute IRI.
pub async fn apply_expand_values<H: BuildHasher + Sync>(
    filter: Filter<Expanded>,
    expand_attributes: &HashSet<AttributeIri, H>,
    expander: &impl TermExpander,
) -> Result<Filter<Expanded>, QueryError> {
    walk_terms(filter, &|term| expand_equality_values(term, expand_attributes, expander)).await
}

/// Rewrite every query term of a filter, leaving its boolean structure
/// intact.
async fn walk_terms<S, T, F, Fut>(filter: Filter<S>, handler: &F) -> Result<Filter<T>, QueryError>
where
    S: NameStage,
    T: NameStage,
    F: Fn(QueryTerm<S>) -> Fut + Sync,
    Fut: Future<Output = Result<QueryTerm<T>, QueryError>> + Send,
{
    match filter {
        Filter::And(filters) => {
            let mut expanded = Vec::with_capacity(filters.len());
            for filter in filters {
                expanded.push(walk_terms_boxed(filter, handler).await?);
            }
            Ok(Filter::And(expanded))
        }
        Filter::Or(filters) => {
            let mut expanded = Vec::with_capacity(filters.len());
            for filter in filters {
                expanded.push(walk_terms_boxed(filter, handler).await?);
            }
            Ok(Filter::Or(expanded))
        }
        Filter::Term(term) => Ok(Filter::Term(handler(term).await?)),
    }
}

/// Box-pinned wrapper: the walker recurses, and an `async fn` cannot name its
/// own future type.
fn walk_terms_boxed<'a, S, T, F, Fut>(filter: Filter<S>, handler: &'a F) -> Pin<Box<dyn Future<Output = Result<Filter<T>, QueryError>> + Send + 'a>>
where
    S: NameStage,
    T: NameStage,
    F: Fn(QueryTerm<S>) -> Fut + Sync,
    Fut: Future<Output = Result<QueryTerm<T>, QueryError>> + Send + 'a,
{
    Box::pin(walk_terms(filter, handler))
}

/// Expand the attribute path of a single query term.
async fn expand_query_term_paths(term: QueryTerm<Written>, members: NestedNames<'_>, expander: &impl TermExpander) -> Result<QueryTerm<Expanded>, QueryError> {
    match term {
        QueryTerm::Exists(path) => Ok(QueryTerm::Exists(expand_attribute_path(path, members, expander).await?)),
        QueryTerm::Equality { path, membership, value } => Ok(QueryTerm::Equality {
            path: expand_attribute_path(path, members, expander).await?,
            membership,
            value,
        }),
        QueryTerm::Ordering { path, operator, value } => Ok(QueryTerm::Ordering {
            path: expand_attribute_path(path, members, expander).await?,
            operator,
            value,
        }),
        QueryTerm::MatchPattern { path, pattern } => Ok(QueryTerm::MatchPattern {
            path: expand_attribute_path(path, members, expander).await?,
            pattern,
        }),
        QueryTerm::NotMatchPattern { path, pattern } => Ok(QueryTerm::NotMatchPattern {
            path: expand_attribute_path(path, members, expander).await?,
            pattern,
        }),
    }
}

/// Expand an attribute path: every segment of its value path as a term, and for
/// each linked-entity hop the Relationship as a term and its guard as entity
/// types.
///
/// The guard names entity types, not attributes, so it expands through
/// [`TermExpander::expand_type`] (clause 4.9's `EntityType` production, which
/// clause 4.17 expands the same way a `type` selection is expanded). A guard
/// type is held to the identifier bound as a Relationship is.
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`] when a segment, a hop's Relationship or
/// one of its guard types does not expand, carrying
/// [`TermExpansionError::NotAnIri`] when it expands to something that is not an
/// absolute IRI, and [`QueryError::Filter`] carrying
/// [`FilterError::InvalidAttributeName`] when it does not expand to a usable
/// attribute name.
pub async fn expand_attribute_path(
    path: AttributePath<Written>,
    members: NestedNames<'_>,
    expander: &impl TermExpander,
) -> Result<AttributePath<Expanded>, QueryError> {
    match path {
        AttributePath::Local(local) => Ok(AttributePath::Local(expand_value_path(local, members, expander).await?)),
        AttributePath::Linked {
            relationship,
            guard_types,
            inner,
        } => {
            let relationship = AttributeIri::try_from(expander.expand_term(relationship.as_str()).await?).map_err(FilterError::from)?;
            let mut expanded_guard = Vec::with_capacity(guard_types.len());
            for guard_type in &guard_types {
                let iri = expander.expand_type(guard_type.as_str()).await?;
                IdentifierTooLong::check(iri.as_str()).map_err(FilterError::from)?;
                expanded_guard.push(EntityTypeIri::from(iri));
            }
            let inner = expand_attribute_path_boxed(*inner, members, expander).await?;
            Ok(AttributePath::Linked {
                relationship,
                guard_types: expanded_guard,
                inner: Box::new(inner),
            })
        }
    }
}

/// Box-pinned wrapper: the attribute path walker recurses through hops, and an
/// `async fn` cannot name its own future type.
fn expand_attribute_path_boxed<'a, E: TermExpander>(
    path: AttributePath<Written>,
    members: NestedNames<'a>,
    expander: &'a E,
) -> Pin<Box<dyn Future<Output = Result<AttributePath<Expanded>, QueryError>> + Send + 'a>> {
    Box::pin(expand_attribute_path(path, members, expander))
}

/// Expand a value path (attribute and, when `members` makes them terms,
/// sub-attributes and compound-value member names) to full IRIs.
///
/// The member names of a compound value expand for the same reason the attribute name does: JSON-LD expansion of a structured Property value turns its members into IRI-keyed nodes, so `address[city]` addresses the expansion of `city` under the request `@context`, not the four letters the client typed.
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`] when the attribute, one of its
/// sub-attributes or one of its compound-value members does not expand,
/// carrying [`TermExpansionError::NotAnIri`] when it expands to something that
/// is not an absolute IRI, and [`QueryError::Filter`] carrying
/// [`FilterError::InvalidAttributeName`] when it does not expand to a usable
/// attribute name.
pub async fn expand_value_path(path: ValuePath<Written>, members: NestedNames<'_>, expander: &impl TermExpander) -> Result<ValuePath<Expanded>, QueryError> {
    let attribute = AttributeIri::try_from(expander.expand_term(path.attribute.as_str()).await?).map_err(FilterError::from)?;
    let sub_attributes = if members.sub_attributes_are_terms() {
        expand_attribute_names(&path.sub_attributes, expander)
            .await?
            .into_iter()
            .map(AttributeName::from)
            .collect()
    } else {
        path.sub_attributes
    };
    // A value that is raw JSON (an attribute the request declared in `jsonKeys`,
    // clause 4.9 EXAMPLE 11, or any Context Source Property) is
    // addressed by literal object keys, which no `@context` expands.
    let trailing = match path.trailing {
        Some(TrailingGroup::Members(mut member_path)) => {
            if members.are_terms_under(&attribute) {
                member_path.addressed = expand_member_names(&member_path.addressed, expander).await?;
            }
            Some(TrailingGroup::Members(member_path))
        }
        other @ (Some(TrailingGroup::Language(_) | TrailingGroup::AllLanguages) | None) => other,
    };
    Ok(ValuePath {
        attribute,
        sub_attributes,
        trailing,
    })
}

/// Expand the members of a trailing group, in order.
///
/// Clause 4.9 evaluates a trailing group as an ECMA 262 `MemberExpression`
/// against the seed target value, so a member the `@context` maps to a JSON-LD
/// keyword, as the core context maps `type` and `id`, is no IRI-keyed member of
/// an expanded value and is read as the member of its own name.
///
/// # Errors
///
/// Returns the errors of [`expand_attribute_names`] for a member that expands
/// to no keyword.
async fn expand_member_names(members: &[AttributeName], expander: &impl TermExpander) -> Result<Vec<AttributeName>, QueryError> {
    let mut names = Vec::with_capacity(members.len());
    for member in members {
        names.push(match expander.expand_term(member.as_str()).await {
            Ok(iri) => AttributeName::from(AttributeIri::try_from(iri).map_err(FilterError::from)?),
            Err(TermExpansionError::Keyword) => member.clone(),
            Err(other) => return Err(other.into()),
        });
    }
    Ok(names)
}

/// Expand every term of an attribute-name list, in order.
///
/// This is what turns an `expandValues` or `jsonKeys` opt-in list into the
/// names [`apply_expand_values`] and [`JsonKeyAttributes`] key a filter by
/// (clause 4.9): both match against the terminal attribute of the path of an
/// already-expanded query term, so a compact opt-in list would key nothing
/// at all. It is also how a path's sub-attributes are expanded.
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`] when a term does not expand, carrying
/// [`TermExpansionError::NotAnIri`] when it expands to something that is not an
/// absolute IRI, and [`QueryError::Filter`] carrying
/// [`FilterError::InvalidAttributeName`] when a term does not expand to a usable
/// attribute name.
pub async fn expand_attribute_names(terms: &[AttributeName], expander: &impl TermExpander) -> Result<Vec<AttributeIri>, QueryError> {
    let mut expanded_names = Vec::with_capacity(terms.len());
    for term in terms {
        expanded_names.push(AttributeIri::try_from(expander.expand_term(term.as_str()).await?).map_err(FilterError::from)?);
    }
    Ok(expanded_names)
}

/// Expand a request's clause 4.9 `jsonKeys` declaration into the attributes
/// every path of that request reads as raw JSON, the empty set when the
/// request declared none.
///
/// One request has one declaration, so its `q` filter and its `orderBy` or
/// grouping paths all resolve a member under a declared attribute the same
/// way.
///
/// # Errors
///
/// Returns the errors of [`expand_attribute_names`].
pub async fn expand_json_keys(declared: Option<&[AttributeName]>, expander: &impl TermExpander) -> Result<JsonKeyAttributes, QueryError> {
    Ok(match declared {
        Some(names) => expand_attribute_names(names, expander).await?.into_iter().collect(),
        None => JsonKeyAttributes::default(),
    })
}

/// Expand the operands of a comparison when its attribute opted in via
/// `expandValues`.
async fn expand_equality_values<H: BuildHasher + Sync>(
    term: QueryTerm<Expanded>,
    expand_attributes: &HashSet<AttributeIri, H>,
    expander: &impl TermExpander,
) -> Result<QueryTerm<Expanded>, QueryError> {
    if expand_attributes.contains(term.path().terminal().attribute.as_str()) {
        return OperandCoercion::ExpandValues.expand_operands(term, expander).await;
    }
    Ok(term)
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::{
            Filter,
            attribute_path::AttributePath,
            equality_value::EqualityValue,
            expansion::{apply_expand_values, expand_attribute_names, expand_context_source_filter, expand_filter, expand_json_keys},
            filter_error::FilterError,
            json_key_attributes::JsonKeyAttributes,
            query_term::{OrderingOperator, QueryTerm},
            scalar::Scalar,
            value_path::ValuePath,
        },
        name_stage::{Expanded, NameStage, Written},
        namespace_expander::{EXAMPLE_NAMESPACE_TYPED, NamespaceExpander},
        query_error::QueryError,
        term_expander::{TermExpander, TermExpansionError},
    };
    use andromeda_ngsi_ld_lexicon::{
        attribute_iri::AttributeIri,
        attribute_name::AttributeName,
        entity_type_iri::EntityTypeIri,
        identifier_length::IDENTIFIER_BYTE_LIMIT,
        reference::NgsiLdReferenceError,
    };
    use andromeda_ngsi_ld_vocabulary::expanded::properties::{DATASET_ID, OBJECT_TYPE, VALUE_TYPE};
    use iri_rs::{IriBuf, iri};
    use pretty_assertions::assert_eq;
    use std::{
        collections::HashSet,
        future::{Future, ready},
    };

    /// Prefixes every term but the three core members a path can end on, so a
    /// test can see exactly which positions were rewritten.
    const CORE_MEMBER_EXPANDER: NamespaceExpander =
        EXAMPLE_NAMESPACE_TYPED.with_core_members(&[("valueType", VALUE_TYPE), ("objectType", OBJECT_TYPE), ("datasetId", DATASET_ID)]);

    /// Expands `type` to the `@type` keyword, as the core context does, and
    /// every other term as [`CORE_MEMBER_EXPANDER`] does.
    struct KeywordTypeExpander;

    impl TermExpander for KeywordTypeExpander {
        fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, TermExpansionError>> + Send {
            let keyword = term == "type";
            let delegated = CORE_MEMBER_EXPANDER.expand_term(term);
            async move { if keyword { Err(TermExpansionError::Keyword) } else { delegated.await } }
        }

        fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, TermExpansionError>> + Send {
            CORE_MEMBER_EXPANDER.expand_type(term)
        }
    }

    /// Parses and expands a `q` filter, `None` when `q` is absent or
    /// blank.
    async fn parse_and_expand(q: Option<&str>, json_keys: &JsonKeyAttributes, expander: &impl TermExpander) -> Result<Option<Filter<Expanded>>, QueryError> {
        let Some(q_str) = q.map(str::trim).filter(|trimmed| !trimmed.is_empty()) else {
            return Ok(None);
        };
        let filter = q_str.parse::<Filter<Written>>()?;
        Ok(Some(expand_filter(filter, json_keys, expander).await?))
    }

    /// Clause 4.9 reads a trailing group as an ECMA 262 `MemberExpression`, so
    /// a member the `@context` maps to a JSON-LD keyword is the member of its
    /// own name, while an attribute so mapped still names no attribute.
    #[tokio::test]
    async fn a_member_mapped_to_a_keyword_is_read_as_written() {
        let expanded = parse_and_expand(Some("location[type]==\"Point\""), &JsonKeyAttributes::default(), &KeywordTypeExpander)
            .await
            .expect("expands")
            .expect("present");
        let path = path_of(&expanded);
        assert_eq!(path.attribute.as_str(), "https://example.com/location");
        assert_eq!(
            path.members()
                .map(|members| members.addressed.iter().map(AttributeName::as_str).collect::<Vec<_>>()),
            Some(vec!["type"])
        );
        assert!(matches!(
            parse_and_expand(Some("type==\"Point\""), &JsonKeyAttributes::default(), &KeywordTypeExpander).await,
            Err(QueryError::TermExpansion(TermExpansionError::Keyword))
        ));
    }

    fn opted_in(names: &[&str]) -> HashSet<AttributeIri> {
        names
            .iter()
            .map(|name| format!("https://example.com/{name}").parse().expect("an absolute IRI"))
            .collect()
    }

    /// `q` expanded under [`CORE_MEMBER_EXPANDER`].
    async fn expanded(q: &str) -> Filter<Expanded> {
        parse_and_expand(Some(q), &JsonKeyAttributes::default(), &CORE_MEMBER_EXPANDER)
            .await
            .expect("expands")
            .expect("present")
    }

    fn path_of(filter: &Filter<Expanded>) -> &ValuePath<Expanded> {
        match filter {
            Filter::Term(QueryTerm::Equality { path, .. } | QueryTerm::Ordering { path, .. }) => path.terminal(),
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected a comparison, got {other:?}"),
        }
    }

    fn equality_value<S: NameStage>(filter: &Filter<S>) -> &EqualityValue {
        match filter {
            Filter::Term(QueryTerm::Equality { value, .. }) => value,
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected an equality comparison, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_empty_filter_expands_to_nothing() {
        assert!(
            parse_and_expand(None, &JsonKeyAttributes::default(), &CORE_MEMBER_EXPANDER)
                .await
                .expect("no filter")
                .is_none()
        );
        assert!(
            parse_and_expand(Some("   "), &JsonKeyAttributes::default(), &CORE_MEMBER_EXPANDER)
                .await
                .expect("blank filter")
                .is_none()
        );
    }

    #[tokio::test]
    async fn every_path_segment_of_every_term_is_expanded() {
        let expanded = parse_and_expand(
            Some("speed>50;temperature.observedAt<2024-01-01T00:00:00Z"),
            &JsonKeyAttributes::default(),
            &CORE_MEMBER_EXPANDER,
        )
        .await
        .expect("expands")
        .expect("present");
        let Filter::And(filters) = expanded else {
            panic!("expected a conjunction");
        };
        let first = path_of(&filters[0]);
        assert_eq!(first.attribute.as_str(), "https://example.com/speed");
        let second = path_of(&filters[1]);
        assert_eq!(second.attribute.as_str(), "https://example.com/temperature");
        assert_eq!(
            second.sub_attributes.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
            vec!["https://example.com/observedAt"]
        );
    }

    /// A hop's two halves expand through different vocabularies: the
    /// Relationship is an attribute term, the guard names entity types. The stub
    /// expander sends each through a distinguishable prefix so the test can see
    /// that `expand_term` and `expand_type` were applied to the right halves:
    /// swapping them would expand a guard against the attribute vocabulary and
    /// silently stop matching every entity.
    #[tokio::test]
    async fn a_hop_expands_its_relationship_as_a_term_and_its_guard_as_entity_types() {
        let expanded = parse_and_expand(
            Some("isParkedIn{Building,Store:category}==\"x\""),
            &JsonKeyAttributes::default(),
            &CORE_MEMBER_EXPANDER,
        )
        .await
        .expect("expands")
        .expect("present");
        match expanded {
            Filter::Term(QueryTerm::Equality { path, .. }) => match path {
                AttributePath::Linked {
                    relationship,
                    guard_types,
                    inner,
                } => {
                    assert_eq!(relationship.as_str(), "https://example.com/isParkedIn");
                    assert_eq!(
                        guard_types.iter().map(EntityTypeIri::as_str).collect::<Vec<&str>>(),
                        vec!["https://example.com/types/Building", "https://example.com/types/Store"]
                    );
                    assert_eq!(inner.terminal().attribute.as_str(), "https://example.com/category");
                }
                AttributePath::Local(local) => panic!("expected a hop, got {local:?}"),
            },
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected an equality query term, got {other:?}"),
        }
    }

    /// The names of clause 4.9 EXAMPLE 14 under `https://example.org/`.
    const EXAMPLE_ORG: NamespaceExpander = NamespaceExpander::uniform(iri!("https://example.org/"));

    /// A `@context` defining the attributes `sensor` and `humidity` and the entity type `Device` of clause 4.9 EXAMPLE 14, and nothing else, so a type expanded as an attribute or an attribute expanded as a type expands to nothing.
    struct Example14Context;

    impl TermExpander for Example14Context {
        fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, TermExpansionError>> + Send {
            ready(match term {
                "sensor" | "humidity" => EXAMPLE_ORG.term_iri(term),
                _ => Err(TermExpansionError::NotExpanded),
            })
        }

        fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, TermExpansionError>> + Send {
            ready(match term {
                "Device" => EXAMPLE_ORG.type_iri(term),
                _ => Err(TermExpansionError::NotExpanded),
            })
        }
    }

    /// Clause 5.7.2.4 expands type and Attribute names, and a type of a hop's list names an entity type as one of a type selection does (clause 4.17).
    #[tokio::test]
    async fn a_type_list_of_one_type_expands_through_the_entity_type_vocabulary() {
        let expanded = parse_and_expand(Some("sensor{Device:humidity}==40"), &JsonKeyAttributes::default(), &Example14Context)
            .await
            .expect("expands")
            .expect("present");
        let stored = "\"https://example.org/sensor\"{\"https://example.org/Device\":\"https://example.org/humidity\"}==40"
            .parse::<Filter<Written>>()
            .expect("parses")
            .into_expanded()
            .expect("absolute names");
        assert_eq!(expanded, stored);
    }

    /// A `@context` holding the term `speed` and the prefix `ex`, both for `https://example.org/`, expanding each name as
    /// JSON-LD expansion does and nothing else, so a name handed over with its quotes expands to nothing.
    struct PrefixContext;

    impl TermExpander for PrefixContext {
        fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, TermExpansionError>> + Send {
            ready(match term {
                "speed" | "ex:speed" | "https://example.org/speed" => EXAMPLE_ORG.term_iri("speed"),
                _ => Err(TermExpansionError::NotExpanded),
            })
        }

        fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, TermExpansionError>> + Send {
            ready(EXAMPLE_ORG.type_iri(term))
        }
    }

    /// The quotes of a name only delimit it, so the expander is handed the name and expands it as clause 5.5.7 says:
    /// a prefixed name through its prefix, an absolute IRI as itself and a term as the unquoted term.
    #[tokio::test]
    async fn a_quoted_name_expands_as_the_name_it_holds() {
        for q in ["\"ex:speed\">50", "\"https://example.org/speed\">50", "\"speed\">50", "speed>50"] {
            let expanded = parse_and_expand(Some(q), &JsonKeyAttributes::default(), &PrefixContext)
                .await
                .unwrap_or_else(|error| panic!("{q}: {error:?}"))
                .expect("present");
            assert_eq!(path_of(&expanded).attribute.as_str(), "https://example.org/speed", "{q}");
        }
    }

    #[tokio::test]
    async fn nested_hops_are_expanded_at_every_level() {
        let expanded = parse_and_expand(Some("outer{inner{zone}}==\"north\""), &JsonKeyAttributes::default(), &CORE_MEMBER_EXPANDER)
            .await
            .expect("expands")
            .expect("present");
        let reparsed = expanded
            .to_string()
            .parse::<Filter<Written>>()
            .expect("reparses")
            .into_expanded()
            .expect("absolute names");
        assert_eq!(reparsed, expanded, "an expanded traversal must round-trip");
        assert_eq!(path_of(&expanded).attribute.as_str(), "https://example.com/zone");
    }

    #[tokio::test]
    async fn an_opt_in_list_is_expanded_into_the_names_a_filter_is_keyed_by() {
        let names: Vec<AttributeName> = ["colour", "label"].iter().map(|name| name.parse().expect("valid attribute name")).collect();
        let expanded = expand_attribute_names(&names, &CORE_MEMBER_EXPANDER).await.expect("expands");
        assert_eq!(
            expanded.iter().map(AttributeIri::as_str).collect::<Vec<_>>(),
            vec!["https://example.com/colour", "https://example.com/label"]
        );
    }

    /// The opt-in names key query terms whose paths were already expanded, so an
    /// expanded list applied to an expanded filter is what actually rewrites a
    /// value.
    #[tokio::test]
    async fn an_expanded_opt_in_list_keys_an_expanded_filter() {
        let filter = parse_and_expand(Some("colour==\"red\""), &JsonKeyAttributes::default(), &CORE_MEMBER_EXPANDER)
            .await
            .expect("expands")
            .expect("present");
        let names: Vec<AttributeName> = vec!["colour".parse().expect("valid attribute name")];
        let opted_in: HashSet<AttributeIri> = expand_attribute_names(&names, &CORE_MEMBER_EXPANDER)
            .await
            .expect("expands")
            .into_iter()
            .collect();
        let expanded = apply_expand_values(filter, &opted_in, &CORE_MEMBER_EXPANDER).await.expect("expands");
        assert_eq!(
            equality_value(&expanded),
            &EqualityValue::Scalar(Scalar::Str("https://example.com/red".to_string()))
        );
    }

    #[tokio::test]
    async fn expand_values_rewrites_only_the_named_attributes() {
        let filter = expanded("colour==\"red\";label==\"red\"").await;
        let opted_in = opted_in(&["colour"]);
        let expanded = apply_expand_values(filter, &opted_in, &CORE_MEMBER_EXPANDER).await.expect("expands");
        let Filter::And(filters) = expanded else {
            panic!("expected a conjunction");
        };
        assert_eq!(
            equality_value(&filters[0]),
            &EqualityValue::Scalar(Scalar::Str("https://example.com/red".to_string()))
        );
        assert_eq!(equality_value(&filters[1]), &EqualityValue::Scalar(Scalar::Str("red".to_string())));
    }

    #[tokio::test]
    async fn expand_values_leaves_non_string_operands_alone() {
        let filter = expanded("speed==10..20").await;
        let opted_in = opted_in(&["speed"]);
        let expanded = apply_expand_values(filter, &opted_in, &CORE_MEMBER_EXPANDER).await.expect("expands");
        assert_eq!(
            equality_value(&expanded),
            &EqualityValue::Range {
                min: Scalar::Number(10.into()),
                max: Scalar::Number(20.into()),
            }
        );
    }

    #[tokio::test]
    async fn expand_values_rewrites_the_string_members_of_a_value_list() {
        let filter = expanded("colour==\"red\",\"blue\"").await;
        let opted_in = opted_in(&["colour"]);
        let expanded = apply_expand_values(filter, &opted_in, &CORE_MEMBER_EXPANDER).await.expect("expands");
        assert_eq!(
            equality_value(&expanded),
            &EqualityValue::ValueList(vec![
                Scalar::Str("https://example.com/red".to_string()),
                Scalar::Str("https://example.com/blue".to_string()),
            ])
        );
    }

    /// Clause 4.9 derives a bare word that is no `URI` from no production, so
    /// it is `BadRequestData` (clause 5.7.2.4) whatever `expandValues` names,
    /// alone, in a value list, under `!=` and inside a logical combination; the
    /// spelling EXAMPLE 12 writes, `gender==Male`, included.
    #[test]
    fn a_bare_word_is_refused_whatever_expand_values_names() {
        for q in [
            "status==active",
            "status==\"on\",active",
            "status!=active",
            "speed>1;(status==active|name==\"x\")",
            "gender==Male",
            "tier==gold",
        ] {
            let error = q.parse::<Filter<Written>>().expect_err("a bare word is no value");
            assert!(matches!(error, FilterError::BareWord), "{q}: {error:?}");
        }
    }

    /// Clause 6.4.3.2 with clause 4.9: a quoted word of an attribute
    /// `expandValues` names expands to the `URI` it names, alone or in a value
    /// list.
    #[tokio::test]
    async fn expand_values_expands_a_quoted_word_to_the_uri_it_names() {
        let opted_in = opted_in(&["gender"]);
        let list = apply_expand_values(expanded("gender==\"Male\",\"Female\"").await, &opted_in, &CORE_MEMBER_EXPANDER)
            .await
            .expect("expands");
        assert_eq!(
            equality_value(&list),
            &EqualityValue::ValueList(vec![
                Scalar::Str("https://example.com/Male".to_string()),
                Scalar::Str("https://example.com/Female".to_string()),
            ])
        );
    }

    /// A quoted word and a bare `URI` are values whatever `expandValues` names.
    #[tokio::test]
    async fn a_quoted_word_and_a_bare_uri_stand_without_expand_values() {
        for q in ["status==\"active\"", "ref==urn:ngsi-ld:Device:1", "speed>1"] {
            let filter = expanded(q).await;
            let kept = apply_expand_values(filter.clone(), &HashSet::<AttributeIri>::new(), &CORE_MEMBER_EXPANDER)
                .await
                .expect("a value");
            assert_eq!(kept, filter, "{q}");
        }
    }

    /// JSON-LD expansion of a structured Property value turns its members into IRI-keyed nodes, so a compound-value member has to be expanded through the same `@context` the attribute name is.
    #[tokio::test]
    async fn a_compound_value_member_is_expanded_through_the_context() {
        let expanded = parse_and_expand(Some("address[city]==\"Berlin\""), &JsonKeyAttributes::default(), &CORE_MEMBER_EXPANDER)
            .await
            .expect("expands")
            .expect("present");
        let path = path_of(&expanded);
        assert_eq!(path.attribute.as_str(), "https://example.com/address");
        assert_eq!(
            path.members()
                .expect("a member path")
                .addressed
                .iter()
                .map(AttributeName::as_str)
                .collect::<Vec<_>>(),
            vec!["https://example.com/city"]
        );
    }

    #[tokio::test]
    async fn every_member_of_a_nested_compound_path_is_expanded() {
        let expanded = parse_and_expand(
            Some("sensor.rawdata[airquality.particulate]==40"),
            &JsonKeyAttributes::default(),
            &CORE_MEMBER_EXPANDER,
        )
        .await
        .expect("expands")
        .expect("present");
        let path = path_of(&expanded);
        assert_eq!(
            path.sub_attributes.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
            vec!["https://example.com/rawdata"]
        );
        assert_eq!(
            path.members()
                .expect("a member path")
                .addressed
                .iter()
                .map(AttributeName::as_str)
                .collect::<Vec<_>>(),
            vec!["https://example.com/airquality", "https://example.com/particulate"]
        );
    }

    /// The serialisation writes each member in its addressed spelling, the one
    /// another broker reads, so the filter it reparses to writes the same text
    /// and addresses the same members. The written spelling travels in
    /// `Filter::written_member_text` alone.
    #[tokio::test]
    async fn an_expanded_compound_path_round_trips_through_the_serialisation() {
        let expanded = parse_and_expand(Some("address[city]==\"Berlin\""), &JsonKeyAttributes::default(), &CORE_MEMBER_EXPANDER)
            .await
            .expect("expands")
            .expect("present");
        let reparsed = expanded
            .to_string()
            .parse::<Filter<Written>>()
            .expect("reparses")
            .into_expanded()
            .expect("absolute names");
        assert_eq!(reparsed.to_string(), expanded.to_string());
        assert_eq!(
            path_of(&reparsed).members().expect("a member path").addressed,
            path_of(&expanded).members().expect("a member path").addressed
        );
    }

    /// Clause 4.9 EXAMPLE 11: with
    /// `?q=parkingTickets[value]=="Overstay 60 minutes"&jsonKeys=parkingTickets`
    /// "the target value raw is JSON, and is not expanded to ngsi-ld:hasValue
    /// using the core @context", so the member the filter addresses stays the
    /// literal object key.
    #[tokio::test]
    async fn a_json_keys_attribute_keeps_its_compound_value_member_literal() {
        let filter = "parkingTickets[value]==\"Overstay 60 minutes\"".parse::<Filter<Written>>().expect("parses");
        let declared: JsonKeyAttributes = ["https://example.com/parkingTickets".parse().expect("valid attribute name")]
            .into_iter()
            .collect();
        let expanded = expand_filter(filter, &declared, &CORE_MEMBER_EXPANDER).await.expect("expands");
        let path = path_of(&expanded);
        assert_eq!(
            path.attribute.as_str(),
            "https://example.com/parkingTickets",
            "the attribute itself still expands"
        );
        assert_eq!(
            path.members()
                .expect("a member path")
                .addressed
                .iter()
                .map(AttributeName::as_str)
                .collect::<Vec<_>>(),
            vec!["value"]
        );
    }

    /// The same filter without the declaration expands the member, which is
    /// what makes the declaration observable at all.
    #[tokio::test]
    async fn the_same_member_expands_when_no_json_keys_declaration_names_the_attribute() {
        let filter = "parkingTickets[value]==\"Overstay 60 minutes\"".parse::<Filter<Written>>().expect("parses");
        let declared: JsonKeyAttributes = ["https://example.com/other".parse().expect("valid attribute name")].into_iter().collect();
        let expanded = expand_filter(filter, &declared, &CORE_MEMBER_EXPANDER).await.expect("expands");
        assert_eq!(
            path_of(&expanded)
                .members()
                .expect("a member path")
                .addressed
                .iter()
                .map(AttributeName::as_str)
                .collect::<Vec<_>>(),
            vec!["https://example.com/value"]
        );
    }

    #[tokio::test]
    async fn a_json_keys_declaration_names_the_expanded_attributes() {
        let names = ["parkingTickets".parse().expect("valid attribute name")];
        let declared = expand_json_keys(Some(&names), &CORE_MEMBER_EXPANDER).await.expect("expands");
        assert!(declared.contains(&"https://example.com/parkingTickets".parse().expect("valid attribute name")));
        let only_the_expanded_name = HashSet::from(["https://example.com/parkingTickets".parse::<AttributeIri>().expect("valid attribute name")]);
        assert_eq!(
            declared,
            JsonKeyAttributes::from(only_the_expanded_name),
            "the compact name is no key of the expanded paths"
        );
    }

    /// A term that is itself a written name, and that every namespace expansion takes past the identifier bound.
    fn oversize_term() -> String {
        "a".repeat(IDENTIFIER_BYTE_LIMIT - 4)
    }

    /// Clause 4.6.2 bounds every name the broker keys by, and an expansion may lengthen a name, so an expanded name
    /// past the bound is refused as an invalid attribute name wherever a filter names one.
    #[tokio::test]
    async fn an_expanded_name_past_the_identifier_bound_is_refused_wherever_the_filter_names_one() {
        let long = oversize_term();
        for (position, q) in [
            ("attribute", format!("{long}>1")),
            ("sub-attribute", format!("speed.{long}>1")),
            ("compound-value member", format!("speed[{long}]==\"x\"")),
            ("hop relationship", format!("{long}{{Building:category}}==\"x\"")),
        ] {
            let refused = parse_and_expand(Some(&q), &JsonKeyAttributes::default(), &CORE_MEMBER_EXPANDER).await;

            assert!(
                matches!(
                    refused,
                    Err(QueryError::Filter(FilterError::InvalidAttributeName(NgsiLdReferenceError::TooLong(_))))
                ),
                "{position}: {refused:?}"
            );
        }
    }

    /// A hop's guard type is held to the same bound as the Relationship it guards.
    #[tokio::test]
    async fn an_expanded_hop_guard_type_past_the_identifier_bound_is_refused() {
        let q = format!("isParkedIn{{{}:category}}==\"x\"", oversize_term());

        let refused = parse_and_expand(Some(&q), &JsonKeyAttributes::default(), &CORE_MEMBER_EXPANDER).await;

        assert!(
            matches!(
                refused,
                Err(QueryError::Filter(FilterError::InvalidAttributeName(NgsiLdReferenceError::TooLong(_))))
            ),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn a_request_without_json_keys_declares_no_attribute() {
        assert!(expand_json_keys(None, &CORE_MEMBER_EXPANDER).await.expect("expands").is_empty());
    }

    /// A declared attribute's NGSI-LD sub-attributes are not object keys, so
    /// they keep expanding through the `@context`.
    #[tokio::test]
    async fn a_json_keys_attribute_still_expands_its_ngsi_ld_sub_attributes() {
        let filter = "parkingTickets.observedAt<2024-01-01T00:00:00Z".parse::<Filter<Written>>().expect("parses");
        let declared: JsonKeyAttributes = ["https://example.com/parkingTickets".parse().expect("valid attribute name")]
            .into_iter()
            .collect();
        let expanded = expand_filter(filter, &declared, &CORE_MEMBER_EXPANDER).await.expect("expands");
        assert_eq!(
            path_of(&expanded).sub_attributes.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
            vec!["https://example.com/observedAt"]
        );
    }

    /// Clause 4.9: a `csf` targets the Context Source Properties of a
    /// registration, held under their expanded names with values that are the
    /// raw JSON the registrant wrote, so the property expands while its
    /// sub-properties and trailing members address the literal keys.
    #[tokio::test]
    async fn a_csf_expands_its_property_and_keeps_its_sub_properties_and_trailing_members_verbatim() {
        let filter = "coverage.since[area.region]==\"north\"".parse::<Filter<Written>>().expect("parses");
        let expanded = expand_context_source_filter(filter, &CORE_MEMBER_EXPANDER).await.expect("expands");
        let path = path_of(&expanded);
        assert_eq!(path.attribute.as_str(), "https://example.com/coverage");
        assert_eq!(path.sub_attributes.iter().map(AttributeName::as_str).collect::<Vec<_>>(), vec!["since"]);
        assert_eq!(
            path.members()
                .expect("a member path")
                .addressed
                .iter()
                .map(AttributeName::as_str)
                .collect::<Vec<_>>(),
            vec!["area", "region"]
        );
        assert_eq!(
            path.members()
                .expect("a member path")
                .written
                .iter()
                .map(AttributeName::as_str)
                .collect::<Vec<_>>(),
            vec!["area", "region"]
        );
        let reparsed = expanded
            .to_string()
            .parse::<Filter<Written>>()
            .expect("reparses")
            .into_expanded()
            .expect("absolute names");
        assert_eq!(reparsed, expanded, "the stored text keeps the members as written");
    }

    /// A `csf` keeps an instance member's core `@context` term as written, so
    /// the stored and forwarded filter is the one the client wrote; its
    /// reading names the member.
    #[tokio::test]
    async fn a_csf_keeps_an_instance_member_term_as_written() {
        let expanded = expand_context_source_filter("tier.unitCode==\"C62\"".parse::<Filter<Written>>().expect("parses"), &CORE_MEMBER_EXPANDER)
            .await
            .expect("expands");
        let path = path_of(&expanded);
        assert_eq!(path.sub_attributes.iter().map(AttributeName::as_str).collect::<Vec<_>>(), vec!["unitCode"]);
        let stored = expanded.to_string();
        assert!(stored.ends_with(".unitCode==\"C62\""), "{stored}");
        assert_eq!(
            stored.parse::<Filter<Written>>().expect("reparses").into_expanded().expect("absolute names"),
            expanded
        );
    }

    #[tokio::test]
    async fn a_pattern_term_keeps_its_pattern_while_its_path_expands() {
        let expanded = parse_and_expand(Some("name~=\"^A.*\""), &JsonKeyAttributes::default(), &CORE_MEMBER_EXPANDER)
            .await
            .expect("expands")
            .expect("present");
        match expanded {
            Filter::Term(QueryTerm::MatchPattern { path, pattern }) => {
                assert_eq!(path.terminal().attribute.as_str(), "https://example.com/name");
                assert_eq!(pattern.as_str(), "^A.*");
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected a pattern query term, got {other:?}"),
        }
    }

    /// Clauses 4.5.2 and 4.5.3: the core `@context` coerces `valueType` and
    /// `objectType` with `@vocab`, so an instance holds each expanded and an
    /// operand compared with it expands as a vocabulary term, alone, in a
    /// value list and under an ordering.
    #[tokio::test]
    async fn an_operand_on_a_value_type_or_object_type_expands_as_a_vocabulary_term() {
        for (q, expected) in [
            ("reading.valueType==\"String\"", "https://example.com/types/String"),
            ("reading.objectType==\"Device\"", "https://example.com/types/Device"),
            ("reading.valueType>\"S\"", "https://example.com/types/S"),
        ] {
            let expanded = parse_and_expand(Some(q), &JsonKeyAttributes::default(), &CORE_MEMBER_EXPANDER)
                .await
                .expect("expands")
                .expect("present");
            let operand = match &expanded {
                Filter::Term(
                    QueryTerm::Equality {
                        value: EqualityValue::Scalar(scalar),
                        ..
                    }
                    | QueryTerm::Ordering { value: scalar, .. },
                ) => scalar,
                other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected one comparison, got {other:?}"),
            };
            assert_eq!(operand, &Scalar::Str(expected.to_owned()), "{q}");
        }
        let list = parse_and_expand(
            Some("reading.objectType==\"Device\",\"Sensor\""),
            &JsonKeyAttributes::default(),
            &CORE_MEMBER_EXPANDER,
        )
        .await
        .expect("expands")
        .expect("present");
        assert_eq!(
            equality_value(&list),
            &EqualityValue::ValueList(vec![
                Scalar::Str("https://example.com/types/Device".to_owned()),
                Scalar::Str("https://example.com/types/Sensor".to_owned()),
            ])
        );
    }

    /// A `datasetId` is no vocabulary term, and a sub-attribute that only
    /// spells like a member is no member.
    #[tokio::test]
    async fn an_operand_on_any_other_target_stays_a_literal() {
        for q in ["reading.datasetId==\"urn:ngsi-ld:Dataset:d1\"", "reading.kind==\"String\""] {
            let expanded = parse_and_expand(Some(q), &JsonKeyAttributes::default(), &CORE_MEMBER_EXPANDER)
                .await
                .expect("expands")
                .expect("present");
            assert_eq!(equality_value(&expanded), equality_value(&q.parse::<Filter<Written>>().expect("parses")), "{q}");
        }
    }

    /// A pattern is a regular expression, never a term.
    #[tokio::test]
    async fn a_pattern_on_a_value_type_stays_as_written() {
        let expanded = parse_and_expand(Some("reading.valueType~=\"Str\""), &JsonKeyAttributes::default(), &CORE_MEMBER_EXPANDER)
            .await
            .expect("expands")
            .expect("present");
        let Filter::Term(QueryTerm::MatchPattern { pattern, .. }) = expanded else {
            panic!("a pattern term");
        };
        assert_eq!(pattern.as_str(), "Str");
    }

    #[tokio::test]
    async fn the_comparison_operator_survives_expansion() {
        let expanded = parse_and_expand(Some("speed>=50"), &JsonKeyAttributes::default(), &CORE_MEMBER_EXPANDER)
            .await
            .expect("expands")
            .expect("present");
        match expanded {
            Filter::Term(QueryTerm::Ordering { operator, .. }) => assert_eq!(operator, OrderingOperator::GreaterOrEqual),
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected a comparison, got {other:?}"),
        }
    }
}

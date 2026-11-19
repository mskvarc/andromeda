//! Reading a filter whose names are already absolute IRIs as an expanded one.
//!
//! A filter is stored, forwarded and written back as its clause 4.9 text, and
//! the text of an expanded filter spells every attribute name and entity type
//! as the absolute IRI it expanded to. Parsing that text yields the written
//! stage, since the text itself says nothing of its stage; the conversion here
//! is the check that makes it the expanded stage, and what refuses a text that
//! names a term where an IRI had to stand. The sub-attributes and the members
//! of a trailing group are not checked: a Context Source filter keeps them as
//! the literal keys its client wrote.
//!
//! A filter whose names follow a rule of their own, such as the members of a
//! resource a filter selects, is converted into the expanded stage by that rule
//! through [`Filter::into_expanded_by`].

use crate::{
    filter::{Filter, attribute_path::AttributePath, query_term::QueryTerm, value_path::ValuePath},
    name_stage::{Expanded, Written},
    query_error::QueryError,
    term_expander::TermExpansionError,
};
use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, attribute_name::AttributeName, entity_type_iri::EntityTypeIri, entity_type_name::EntityTypeName};

/// A written attribute name as the absolute IRI it has to be.
///
/// A written name is non-empty and within the identifier bound, so the only
/// way the IRI check refuses it is that it is no absolute IRI.
fn absolute_attribute(name: &AttributeName) -> Result<AttributeIri, TermExpansionError> {
    name.as_str().parse().map_err(|_not_absolute| TermExpansionError::NotAnIri)
}

/// A written entity type name as the absolute IRI it has to be.
fn absolute_entity_type(name: &EntityTypeName) -> Result<EntityTypeIri, TermExpansionError> {
    name.as_str().parse().map_err(|_not_absolute| TermExpansionError::NotAnIri)
}

impl Filter<Written> {
    /// This filter as an expanded one, its names being absolute IRIs already.
    ///
    /// # Errors
    ///
    /// Returns [`TermExpansionError::NotAnIri`] when an attribute, a hop's
    /// Relationship or one of its guard types is no absolute IRI.
    pub fn into_expanded(self) -> Result<Filter<Expanded>, TermExpansionError> {
        self.into_expanded_by(&absolute_attribute, &absolute_entity_type)
    }

    /// This filter as an expanded one, each attribute name and entity type
    /// named by the rule given.
    ///
    /// # Errors
    ///
    /// Returns the first refusal of `attribute` or `entity_type`.
    pub fn into_expanded_by<E>(
        self,
        attribute: &impl Fn(&AttributeName) -> Result<AttributeIri, E>,
        entity_type: &impl Fn(&EntityTypeName) -> Result<EntityTypeIri, E>,
    ) -> Result<Filter<Expanded>, E> {
        match self {
            Filter::And(filters) => Ok(Filter::And(
                filters
                    .into_iter()
                    .map(|filter| filter.into_expanded_by(attribute, entity_type))
                    .collect::<Result<_, _>>()?,
            )),
            Filter::Or(filters) => Ok(Filter::Or(
                filters
                    .into_iter()
                    .map(|filter| filter.into_expanded_by(attribute, entity_type))
                    .collect::<Result<_, _>>()?,
            )),
            Filter::Term(term) => Ok(Filter::Term(term.into_expanded_by(attribute, entity_type)?)),
        }
    }
}

impl TryFrom<String> for Filter<Expanded> {
    type Error = QueryError;

    // Hand-written: the text of an expanded filter is parsed as the written
    // stage and then checked, which no derive expresses.
    fn try_from(raw: String) -> Result<Filter<Expanded>, QueryError> {
        Ok(Filter::<Written>::try_from(raw)?.into_expanded()?)
    }
}

impl QueryTerm<Written> {
    /// This query term as an expanded one, each name converted by the rule
    /// given.
    ///
    /// # Errors
    ///
    /// As [`Filter::into_expanded_by`].
    pub fn into_expanded_by<E>(
        self,
        attribute: &impl Fn(&AttributeName) -> Result<AttributeIri, E>,
        entity_type: &impl Fn(&EntityTypeName) -> Result<EntityTypeIri, E>,
    ) -> Result<QueryTerm<Expanded>, E> {
        Ok(match self {
            QueryTerm::Exists(path) => QueryTerm::Exists(path.into_expanded_by(attribute, entity_type)?),
            QueryTerm::Equality { path, membership, value } => QueryTerm::Equality {
                path: path.into_expanded_by(attribute, entity_type)?,
                membership,
                value,
            },
            QueryTerm::Ordering { path, operator, value } => QueryTerm::Ordering {
                path: path.into_expanded_by(attribute, entity_type)?,
                operator,
                value,
            },
            QueryTerm::MatchPattern { path, pattern } => QueryTerm::MatchPattern {
                path: path.into_expanded_by(attribute, entity_type)?,
                pattern,
            },
            QueryTerm::NotMatchPattern { path, pattern } => QueryTerm::NotMatchPattern {
                path: path.into_expanded_by(attribute, entity_type)?,
                pattern,
            },
        })
    }
}

impl AttributePath<Written> {
    /// This attribute path as an expanded one, each name converted by the rule
    /// given.
    ///
    /// # Errors
    ///
    /// As [`Filter::into_expanded_by`].
    pub fn into_expanded_by<E>(
        self,
        attribute: &impl Fn(&AttributeName) -> Result<AttributeIri, E>,
        entity_type: &impl Fn(&EntityTypeName) -> Result<EntityTypeIri, E>,
    ) -> Result<AttributePath<Expanded>, E> {
        match self {
            AttributePath::Local(local) => Ok(AttributePath::Local(local.into_expanded_by(attribute)?)),
            AttributePath::Linked {
                relationship,
                guard_types,
                inner,
            } => Ok(AttributePath::Linked {
                relationship: attribute(&relationship)?,
                guard_types: guard_types.iter().map(entity_type).collect::<Result<_, _>>()?,
                inner: Box::new(inner.into_expanded_by(attribute, entity_type)?),
            }),
        }
    }
}

impl ValuePath<Written> {
    /// This value path as an expanded one, its attribute being an absolute IRI
    /// already.
    ///
    /// # Errors
    ///
    /// Returns [`TermExpansionError::NotAnIri`] when the attribute is no
    /// absolute IRI.
    pub fn into_expanded(self) -> Result<ValuePath<Expanded>, TermExpansionError> {
        self.into_expanded_by(&absolute_attribute)
    }

    /// This value path as an expanded one, its attribute converted by the rule
    /// given.
    ///
    /// # Errors
    ///
    /// Returns the refusal of `attribute`.
    pub fn into_expanded_by<E>(self, attribute: &impl Fn(&AttributeName) -> Result<AttributeIri, E>) -> Result<ValuePath<Expanded>, E> {
        Ok(ValuePath {
            attribute: attribute(&self.attribute)?,
            sub_attributes: self.sub_attributes,
            trailing: self.trailing,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::{Filter, attribute_path::AttributePath, query_term::QueryTerm},
        name_stage::{Expanded, Written},
        query_error::QueryError,
        term_expander::TermExpansionError,
    };
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;

    fn expanded(text: &str) -> Filter<Expanded> {
        text.parse::<Filter<Written>>().expect("a filter").into_expanded().expect("absolute names")
    }

    #[test]
    fn an_expanded_filter_round_trips_through_its_text() {
        let filter = expanded(
            "\"https://example.com/isParkedIn\"{\"https://example.com/types/Lot\":\"https://example.com/name\"}==\"x\";\"https://example.com/speed\".\"https://example.com/accuracy\">1",
        );
        let reparsed = filter
            .to_string()
            .parse::<Filter<Written>>()
            .expect("the text parses")
            .into_expanded()
            .expect("absolute names");
        pretty_assertions::assert_eq!(reparsed, filter);
    }

    #[test]
    fn a_filter_naming_a_bare_term_is_not_an_expanded_filter() {
        for text in ["temperature>1", "\"https://example.com/a\"==1;b==2", "\"https://example.com/r\"{name}==\"x\""] {
            let refused = text.parse::<Filter<Written>>().expect("a filter").into_expanded();
            pretty_assertions::assert_eq!(refused, Err(TermExpansionError::NotAnIri), "{text}");
        }
    }

    #[test]
    fn a_guard_naming_a_bare_term_is_not_an_expanded_filter() {
        let refused = "\"https://example.com/r\"{Lot:\"https://example.com/name\"}==\"x\""
            .parse::<Filter<Written>>()
            .expect("a filter")
            .into_expanded();
        pretty_assertions::assert_eq!(refused, Err(TermExpansionError::NotAnIri));
    }

    #[test]
    fn a_guard_of_the_written_stage_is_an_entity_type_name() {
        let Filter::Term(QueryTerm::Exists(AttributePath::Linked { guard_types, .. })) = "r{Lot,Garage:name}".parse::<Filter<Written>>().expect("a filter")
        else {
            panic!("a guarded hop");
        };
        pretty_assertions::assert_eq!(
            guard_types,
            vec![
                "Lot".parse::<EntityTypeName>().expect("a type name"),
                "Garage".parse::<EntityTypeName>().expect("a type name")
            ]
        );
    }

    #[test]
    fn a_stored_expanded_filter_deserialises_from_its_text() {
        let text = "\"https://example.com/speed\">1";
        let filter: Filter<Expanded> = serde_json::from_str(&format!("{text:?}")).expect("an expanded filter");
        pretty_assertions::assert_eq!(filter, expanded(text));
        pretty_assertions::assert_eq!(serde_json::to_string(&filter).expect("serialises"), format!("{text:?}"));
    }

    #[test]
    fn a_stored_filter_naming_a_bare_term_is_refused_as_an_expanded_filter() {
        let refused = Filter::<Expanded>::try_from("speed>1".to_owned());
        assert!(matches!(refused, Err(QueryError::TermExpansion(TermExpansionError::NotAnIri))), "{refused:?}");
    }
}

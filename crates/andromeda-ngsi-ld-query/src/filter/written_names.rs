//! Reading an expanded filter as a written one, its names spelled as the absolute IRIs they are.
//!
//! An absolute IRI is a valid written attribute name, so the conversion keeps the tree and its trailing members as they are. Only a guard type can be refused: an expanded entity type carries no length bound, and a written one does.

use crate::{
    filter::{Filter, attribute_path::AttributePath, query_term::QueryTerm, value_path::ValuePath},
    name_stage::{Expanded, Written},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, reference::NgsiLdReferenceError};

impl Filter<Expanded> {
    /// This filter in the written stage, every name spelled as its absolute IRI and every trailing member kept as it is.
    ///
    /// # Errors
    ///
    /// Returns the refusal of the entity type name grammar for a guard type past the identifier bound.
    pub fn into_written(self) -> Result<Filter<Written>, NgsiLdReferenceError> {
        match self {
            Filter::And(filters) => Ok(Filter::And(filters.into_iter().map(Filter::into_written).collect::<Result<_, _>>()?)),
            Filter::Or(filters) => Ok(Filter::Or(filters.into_iter().map(Filter::into_written).collect::<Result<_, _>>()?)),
            Filter::Term(term) => Ok(Filter::Term(term.into_written()?)),
        }
    }
}

impl QueryTerm<Expanded> {
    /// This query term in the written stage.
    ///
    /// # Errors
    ///
    /// As [`Filter::into_written`].
    pub fn into_written(self) -> Result<QueryTerm<Written>, NgsiLdReferenceError> {
        Ok(match self {
            QueryTerm::Exists(path) => QueryTerm::Exists(path.into_written()?),
            QueryTerm::Equality { path, membership, value } => QueryTerm::Equality {
                path: path.into_written()?,
                membership,
                value,
            },
            QueryTerm::Ordering { path, operator, value } => QueryTerm::Ordering {
                path: path.into_written()?,
                operator,
                value,
            },
            QueryTerm::MatchPattern { path, pattern } => QueryTerm::MatchPattern {
                path: path.into_written()?,
                pattern,
            },
            QueryTerm::NotMatchPattern { path, pattern } => QueryTerm::NotMatchPattern {
                path: path.into_written()?,
                pattern,
            },
        })
    }
}

impl AttributePath<Expanded> {
    /// This attribute path in the written stage.
    ///
    /// # Errors
    ///
    /// As [`Filter::into_written`].
    pub fn into_written(self) -> Result<AttributePath<Written>, NgsiLdReferenceError> {
        match self {
            AttributePath::Local(local) => Ok(AttributePath::Local(local.into_written())),
            AttributePath::Linked {
                relationship,
                guard_types,
                inner,
            } => Ok(AttributePath::Linked {
                relationship: AttributeName::from(relationship),
                guard_types: guard_types
                    .iter()
                    .map(|guard| guard.as_str().parse::<EntityTypeName>())
                    .collect::<Result<_, _>>()?,
                inner: Box::new(inner.into_written()?),
            }),
        }
    }
}

impl ValuePath<Expanded> {
    /// This value path in the written stage.
    #[must_use]
    pub fn into_written(self) -> ValuePath<Written> {
        ValuePath {
            attribute: AttributeName::from(self.attribute),
            sub_attributes: self.sub_attributes,
            trailing: self.trailing,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::Filter,
        name_stage::{Expanded, Written},
    };

    #[test]
    fn an_expanded_filter_reads_as_the_written_filter_of_the_same_text() {
        let text = "\"https://example.com/speed\">10;\"https://example.com/r\"{\"https://example.com/Lot\":\"https://example.com/name\"}==\"x\"";
        let expanded = Filter::<Expanded>::try_from(text.to_owned()).expect("an expanded filter");
        let written = expanded.clone().into_written().expect("every name is within the bound");
        pretty_assertions::assert_eq!(written.to_string(), expanded.to_string());
        pretty_assertions::assert_eq!(written, text.parse::<Filter<Written>>().expect("parses as written"));
    }
}

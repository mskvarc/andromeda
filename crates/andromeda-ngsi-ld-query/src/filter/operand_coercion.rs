//! The `q` operands that are terms rather than literals, expanded before a
//! match runs and compacted back when a stored filter is read.
//!
//! Two rules make an operand a term. `expandValues` names the attributes whose
//! string operands expand under the request `@context` (clause 6.4.3.2). A
//! path ending on `valueType` or `objectType` compares with a member the core
//! `@context` coerces with `@vocab` (clauses 4.5.2 and 4.5.3), which an
//! instance holds expanded, so its operand expands the way an entity type
//! does. Each rule compacts by its own inverse, so a filter read back carries
//! the operands as the client wrote them.

use crate::{
    filter::{equality_value::EqualityValue, query_term::QueryTerm, scalar::Scalar},
    query_error::QueryError,
    term_compactor::TermCompactor,
    term_expander::TermExpander,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use iri_rs::{Iri, IriBuf};

/// Which rule makes the operands of a term terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperandCoercion {
    /// The term's attribute is named by `expandValues`: an operand expands as
    /// an attribute term.
    ExpandValues,
    /// The term's path ends on `valueType` or `objectType`: an operand expands
    /// as a vocabulary-relative IRI, the way an entity type does.
    Vocabulary,
}

impl OperandCoercion {
    /// `term` with the operands of an equality or an ordering expanded; a
    /// pattern is no term and existence has no operand.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::TermExpansion`] or [`QueryError::TermNotAnIri`]
    /// when an operand does not expand.
    pub async fn expand_operands(self, term: QueryTerm, expander: &impl TermExpander) -> Result<QueryTerm, QueryError> {
        Ok(match term {
            QueryTerm::Equality { path, membership, value } => QueryTerm::Equality {
                value: self.expand_equality_value(value, expander).await?,
                path,
                membership,
            },
            QueryTerm::Ordering { path, operator, value } => QueryTerm::Ordering {
                value: self.expand_scalar(value, expander).await?,
                path,
                operator,
            },
            QueryTerm::Exists(_) | QueryTerm::MatchPattern { .. } | QueryTerm::NotMatchPattern { .. } => term,
        })
    }

    /// `term` with the operands [`OperandCoercion::expand_operands`] expanded
    /// put back in the terms of the `@context` `compactor` holds.
    pub async fn compact_operands(self, term: QueryTerm, compactor: &impl TermCompactor) -> QueryTerm {
        match term {
            QueryTerm::Equality { path, membership, value } => QueryTerm::Equality {
                value: self.compact_equality_value(value, compactor).await,
                path,
                membership,
            },
            QueryTerm::Ordering { path, operator, value } => QueryTerm::Ordering {
                value: self.compact_scalar(value, compactor).await,
                path,
                operator,
            },
            QueryTerm::Exists(_) | QueryTerm::MatchPattern { .. } | QueryTerm::NotMatchPattern { .. } => term,
        }
    }

    /// Expand the operands of an equality; a range is bounded by two ordered
    /// values, neither of which is a term.
    async fn expand_equality_value(self, value: EqualityValue, expander: &impl TermExpander) -> Result<EqualityValue, QueryError> {
        match value {
            EqualityValue::Scalar(scalar) => Ok(EqualityValue::Scalar(self.expand_scalar(scalar, expander).await?)),
            EqualityValue::ValueList(values) => {
                let mut scalars = Vec::with_capacity(values.len());
                for scalar in values {
                    scalars.push(self.expand_scalar(scalar, expander).await?);
                }
                Ok(EqualityValue::ValueList(scalars))
            }
            EqualityValue::Range { .. } => Ok(value),
        }
    }

    /// Expand one operand: a string or a `URI` is a term, every other scalar
    /// kind is already a value.
    async fn expand_scalar(self, scalar: Scalar, expander: &impl TermExpander) -> Result<Scalar, QueryError> {
        match scalar {
            Scalar::Str(term) => Ok(Scalar::Str(self.expand(&term, expander).await?.to_string())),
            Scalar::Uri(term) => Ok(Scalar::Uri(self.expand(&term, expander).await?.to_string())),
            other @ (Scalar::Number(_) | Scalar::Bool(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_)) => Ok(other),
        }
    }

    /// One term expanded under the rule.
    async fn expand(self, term: &str, expander: &impl TermExpander) -> Result<IriBuf, QueryError> {
        Ok(match self {
            OperandCoercion::ExpandValues => expander.expand_term(term).await?,
            OperandCoercion::Vocabulary => expander.expand_type(term).await?,
        })
    }

    /// Compact the operands of an equality; a range is left as it is.
    async fn compact_equality_value(self, value: EqualityValue, compactor: &impl TermCompactor) -> EqualityValue {
        match value {
            EqualityValue::Scalar(scalar) => EqualityValue::Scalar(self.compact_scalar(scalar, compactor).await),
            EqualityValue::ValueList(values) => {
                let mut compacted = Vec::with_capacity(values.len());
                for scalar in values {
                    compacted.push(self.compact_scalar(scalar, compactor).await);
                }
                EqualityValue::ValueList(compacted)
            }
            EqualityValue::Range { .. } => value,
        }
    }

    /// Compact one operand.
    ///
    /// Only a string or a `URI` can have been expanded. A string the expansion
    /// could not have produced (one that is no usable name) is the client's own
    /// literal and is left alone.
    async fn compact_scalar(self, scalar: Scalar, compactor: &impl TermCompactor) -> Scalar {
        match scalar {
            Scalar::Str(text) => match text.parse::<AttributeName>() {
                Ok(expanded) => Scalar::Str(self.compact(&expanded, compactor).await.as_str().to_owned()),
                Err(_not_a_name) => Scalar::Str(text),
            },
            Scalar::Uri(text) => match text.parse::<AttributeName>() {
                Ok(expanded) => {
                    let compact = self.compact(&expanded, compactor).await.as_str().to_owned();
                    // A term the compaction leaves is no `URI`, and clause 4.9
                    // writes such a value only as a `quotedStr`.
                    if Iri::new(compact.as_str()).is_ok() {
                        Scalar::Uri(compact)
                    } else {
                        Scalar::Str(compact)
                    }
                }
                Err(_not_a_name) => Scalar::Uri(text),
            },
            other @ (Scalar::Number(_) | Scalar::Bool(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_)) => other,
        }
    }

    /// One expanded name compacted under the rule.
    async fn compact(self, name: &AttributeName, compactor: &impl TermCompactor) -> AttributeName {
        match self {
            OperandCoercion::ExpandValues => compactor.compact_term(name).await,
            OperandCoercion::Vocabulary => compactor.compact_type(name).await,
        }
    }
}

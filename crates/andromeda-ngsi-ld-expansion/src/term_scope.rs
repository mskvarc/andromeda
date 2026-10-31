//! The active context one part of a request is expanded under, and the IRI
//! expansion JSON-LD 1.1 performs against it.
//!
//! Every decision the typed walk makes about a term is the processor's own:
//! keys, `@id` values, `@type` values and `@vocab`-coerced values are all
//! expanded by `expand_iri_simple` over the processed context, exactly the call
//! the processor's expansion algorithm makes for them. The scope also says
//! whether the part it covers is a value the write's value count reaches
//! ([`crate::value_count`]).

use crate::{decline::Decline, value_count::ValueCount};
use andromeda_jsonld::engine::expansion_context::ExpansionContext;
use andromeda_ngsi_ld_number::json_value::value_census::ValueTally;
use iri_rs::IriBuf;
use jsonld::{
    Context,
    Environment,
    Id,
    NoLoader,
    Term,
    ValidId,
    context::TermDefinitionRef,
    context_processing::{
        Warning,
        algorithm::{Action, expand_iri_simple},
    },
    syntax::{ExpandableRef, Nullable},
};
use rdfx::{BlankIdBuf, vocabulary::no_vocabulary_mut};
use std::sync::Arc;

/// One expanded term.
pub(crate) type ExpandedTerm = Arc<Term<IriBuf, BlankIdBuf>>;

/// The active context of one part of a request.
#[derive(Clone, Copy)]
pub(crate) struct TermScope<'c> {
    /// The active context terms are decided against.
    active: &'c Context<IriBuf, BlankIdBuf>,
    /// The request's processed context while the scope is still its root; a
    /// property-scoped context entered on the way leaves `None`, because the
    /// next one would be processed on top of it rather than on the root.
    root: Option<&'c ExpansionContext>,
    /// Where the values expanded in this scope are counted.
    values: ValueCount<'c>,
}

impl<'c> TermScope<'c> {
    /// The root scope of a request.
    ///
    /// # Errors
    ///
    /// Declines a context that does not propagate: JSON-LD 1.1 reverts it on
    /// every node object, which the walk does not model.
    pub(crate) fn root(context: &'c ExpansionContext) -> Result<TermScope<'c>, Decline> {
        let active = context.active();
        if active.previous_context().is_some() {
            return Err(Decline::NonPropagatedContext);
        }
        Ok(TermScope {
            active,
            root: Some(context),
            values: ValueCount::uncounted(),
        })
    }

    /// The same scope, the values of the document it starts counted into
    /// `tally`; `None` counts nothing.
    pub(crate) const fn counting_into<'t>(self, tally: Option<&'t ValueTally>) -> TermScope<'t>
    where
        'c: 't,
    {
        TermScope {
            active: self.active,
            root: self.root,
            values: ValueCount::into_tally(tally),
        }
    }

    /// The same scope, below a Property `value` or a `valueList`, where every
    /// value expanded is counted.
    pub(crate) const fn in_value(&self) -> TermScope<'c> {
        TermScope {
            active: self.active,
            root: self.root,
            values: self.values.in_value(),
        }
    }

    /// Count one value expanded in this scope, when it is a value the count
    /// reaches.
    ///
    /// # Errors
    ///
    /// Returns [`Decline::ValueLimit`] once the count passes the limit.
    pub(crate) fn count_value(&self) -> Result<(), Decline> {
        self.values.count_one()
    }

    /// The room to make for the items an array of `written` elements expands
    /// to in this scope.
    pub(crate) fn value_capacity(&self, written: usize) -> usize {
        self.values.capacity(written)
    }

    /// The definition of `term` in this scope.
    pub(crate) fn definition(&self, term: &str) -> Option<TermDefinitionRef<'c, IriBuf, BlankIdBuf>> {
        self.active.get(term)
    }

    /// The scope an object written under `term` is expanded in: this one, or
    /// the property-scoped context `term` declares (JSON-LD 1.1 expansion
    /// algorithm step 8).
    ///
    /// # Errors
    ///
    /// Declines a scoped context met inside another one, and one that does not
    /// propagate.
    pub(crate) fn object_scope(&self, term: Option<&str>) -> Result<TermScope<'c>, Decline> {
        let Some(term) = term else {
            return Ok(*self);
        };
        if self.definition(term).and_then(|definition| definition.context()).is_none() {
            return Ok(*self);
        }
        let scoped = self.root.and_then(|root| root.property_scoped(term)).ok_or(Decline::NestedScopedContext)?;
        if scoped.previous_context().is_some() {
            return Err(Decline::NonPropagatedContext);
        }
        Ok(TermScope {
            active: scoped,
            root: None,
            values: self.values,
        })
    }

    /// Whether `term` declares a scoped context in this scope.
    pub(crate) fn has_scoped_context(&self, term: &str) -> bool {
        self.definition(term).and_then(|definition| definition.context()).is_some()
    }

    /// The default language of string values in this scope.
    pub(crate) fn default_language(&self) -> Option<String> {
        self.active.default_language().map(|language| language.as_str().to_owned())
    }

    /// The default base direction of string values in this scope.
    pub(crate) fn default_direction(&self) -> Option<jsonld::Direction> {
        self.active.default_base_direction()
    }

    /// Expand a member key (`vocab` on, not document-relative).
    pub(crate) fn expand_key(&self, key: &str) -> Result<ExpandedTerm, Decline> {
        self.expand(key, false, Some(Action::Keep))
    }

    /// Expand an `@id` value or an `@id`-coerced value (document-relative,
    /// `vocab` off).
    pub(crate) fn expand_id(&self, value: &str) -> Result<ExpandedTerm, Decline> {
        self.expand(value, true, None)
    }

    /// Expand an `@type` value or a `@vocab`-coerced value (document-relative,
    /// `vocab` on).
    pub(crate) fn expand_vocab(&self, value: &str) -> Result<ExpandedTerm, Decline> {
        self.expand(value, true, Some(Action::Keep))
    }

    /// The one IRI expansion every other method is.
    fn expand(&self, value: &str, document_relative: bool, vocab: Option<Action>) -> Result<ExpandedTerm, Decline> {
        let mut warnings = ();
        let mut environment = Environment {
            vocabulary: no_vocabulary_mut(),
            loader: &NoLoader,
            warnings: &mut warnings,
        };
        // A keyword written as a key or a value is the keyword, as the
        // processor reads it; expanded as a plain string it would read as a
        // keyword-like term and drop.
        // `Keep` never rejects and never drops, so neither outcome is reached.
        expand_iri_simple::<Warning, _, _, _>(
            &mut environment,
            self.active,
            Nullable::Some(ExpandableRef::from(value)),
            document_relative,
            vocab,
        )
        .ok()
        .flatten()
        .ok_or(Decline::MalformedJsonLd)
    }
}

/// The text of an identifier as expansion leaves it in the expanded document:
/// an IRI, a blank node identifier, or text that names neither.
pub(crate) fn identifier_text(term: &Term<IriBuf, BlankIdBuf>) -> Option<String> {
    match term {
        Term::Null => None,
        Term::Id(id) => Some(id_text(id)),
        Term::Keyword(keyword) => Some(keyword.into_str().to_owned()),
    }
}

/// The text of one identifier.
pub(crate) fn id_text(id: &Id<IriBuf, BlankIdBuf>) -> String {
    match id {
        Id::Valid(ValidId::Iri(iri)) => iri.as_str().to_owned(),
        Id::Valid(ValidId::Blank(blank)) => blank.as_str().to_owned(),
        Id::Invalid(text) => text.clone(), // clone: the identifier is shared by the context's memo
    }
}

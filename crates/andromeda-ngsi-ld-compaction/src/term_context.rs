//! The processed request context, asked one term at a time.
//!
//! Compaction (JSON-LD 1.1 API section 6) decides every key and value of a
//! compacted document against the active context: IRI compaction with term
//! selection (sections 6.2 and 6.2.2) for keys and `@type` values, value
//! compaction (section 6.3) for value objects and node references.
//! [`TermContext`] asks the library those questions directly, without building
//! a document for them, and refuses a selected term whose definition makes
//! compaction depend on more than the term itself (see
//! [`Undecidable`](crate::undecidable::Undecidable)).

use crate::{
    compacted::{Compacted, ProcessorCompacted},
    core_aliases::core_term_aliases,
    undecidable::Undecidable,
};
use andromeda_json_literal::processor_json::read_processor_json;
use andromeda_jsonld::engine::compaction_context::CompactionContext;
use andromeda_ngsi_ld_representation::compact_node::aliased_node::CoreTermAliases;
use dashmap::DashMap;
use jsonld::{
    BlankIdBuf,
    Container,
    ContainerKind,
    Context,
    Id,
    IriBuf,
    Options,
    Term,
    Value,
    compaction::{
        IriConfusedWithPrefix,
        Options as CompactionOptions,
        ValueHint,
        ValueKind,
        compact_iri,
        compact_iri_with_hint,
        compact_node_reference,
        compact_value,
        keyword_alias,
        types_as_array,
        values_as_array,
    },
    rdfx::vocabulary::no_vocabulary,
    syntax::Keyword,
};
use std::sync::Arc;

/// The container kinds whose map the walk does not write.
const MAP_CONTAINERS: [ContainerKind; 4] = [ContainerKind::Index, ContainerKind::Id, ContainerKind::Type, ContainerKind::Graph];

/// One processed context, with the attribute keys one request has selected in
/// it so far.
#[derive(Debug)]
pub(crate) struct TermContext {
    /// The processed context every decision is taken in.
    context: Arc<CompactionContext>,
    /// The options the processor compacts with.
    options: CompactionOptions,
    /// The key each attribute IRI compacted to, for an attribute instance
    /// node: an attribute recurs in every entity of a page, and its key never
    /// depends on the instance.
    attribute_keys: DashMap<Box<str>, Arc<str>>,
    /// The terms of the context that stand for core terms, which a compacted
    /// entity is read through.
    aliases: CoreTermAliases,
}

impl TermContext {
    /// Ask `context` with the options the processor compacts with.
    pub(crate) fn new(context: Arc<CompactionContext>) -> TermContext {
        let aliases = core_term_aliases(context.active_context());
        TermContext {
            context,
            options: Options::<IriBuf>::default().compaction_options(),
            attribute_keys: DashMap::new(),
            aliases,
        }
    }

    /// The terms of the context that stand for core terms.
    pub(crate) const fn aliases(&self) -> &CoreTermAliases {
        &self.aliases
    }

    /// The active context.
    pub(crate) fn context(&self) -> &Context<IriBuf, BlankIdBuf> {
        self.context.active_context()
    }

    /// The key `keyword` is written under: its alias, or its own spelling.
    pub(crate) fn keyword(&self, keyword: Keyword) -> &str {
        keyword_alias(no_vocabulary(), self.context(), self.options, keyword)
    }

    /// The key an attribute is written under for one of its instance nodes.
    ///
    /// An instance node carries no `@id`, so the selection is the same for
    /// every instance of the attribute and is remembered by the attribute's
    /// expanded name.
    pub(crate) fn attribute_key(&self, attribute: &Term<IriBuf, BlankIdBuf>) -> Result<Option<Arc<str>>, Undecidable> {
        let name = attribute.as_str();
        if let Some(key) = self.attribute_keys.get(name) {
            return Ok(Some(Arc::clone(key.value())));
        }
        let key = self.property_key(attribute, ValueHint::new(ValueKind::Node { id: None }))?;
        if let Some(key) = &key {
            self.attribute_keys.insert(Box::from(name), Arc::clone(key));
        }
        Ok(key)
    }

    /// The key `property` is written under for a value of the shape `hint`
    /// describes (JSON-LD 1.1 API section 6.2.2 term selection).
    ///
    /// # Errors
    ///
    /// Returns the [`Undecidable`] reason when the selected term carries a
    /// scoped context, nests its values, or keys them in a map container, or
    /// when the IRI is confused with a prefix.
    pub(crate) fn property_key(&self, property: &Term<IriBuf, BlankIdBuf>, hint: ValueHint<'_, IriBuf, BlankIdBuf>) -> Result<Option<Arc<str>>, Undecidable> {
        let key = compact_iri_with_hint(no_vocabulary(), self.context(), property, hint, true, false, self.options).map_err(confused)?;
        if let Some(key) = &key {
            self.admit_property_term(key)?;
        }
        Ok(key)
    }

    /// What a `@type` value compacts to, refused when it is a term carrying a
    /// type-scoped context.
    pub(crate) fn type_term(&self, ty: &Term<IriBuf, BlankIdBuf>) -> Result<Option<Arc<str>>, Undecidable> {
        let term = compact_iri(no_vocabulary(), self.context(), ty, true, false, self.options).map_err(confused)?;
        let type_scoped = term
            .as_deref()
            .and_then(|term| self.context().get(term))
            .is_some_and(|definition| definition.context().is_some());
        if type_scoped {
            return Err(Undecidable::TypeScopedContext);
        }
        Ok(term)
    }

    /// What an `@id` value compacts to: an IRI reference, relative to the
    /// context's base IRI when it has one.
    pub(crate) fn identifier(&self, id: &Term<IriBuf, BlankIdBuf>) -> Result<Option<Arc<str>>, Undecidable> {
        compact_iri(no_vocabulary(), self.context(), id, false, false, self.options).map_err(confused)
    }

    /// A value object compacted under `key` (JSON-LD 1.1 API section 6.3).
    pub(crate) fn value<'a>(&self, value: &Value<IriBuf>, key: &str) -> Result<Compacted<'a>, Undecidable> {
        compact_value(no_vocabulary(), value, None, self.context(), Some(key), self.options)
            .map(|compacted| read_processor_json(compacted, &mut ProcessorCompacted))
            .map_err(confused)
    }

    /// A node reference compacted under `key`.
    pub(crate) fn node_reference<'a>(&self, id: &Id<IriBuf, BlankIdBuf>, key: &str) -> Result<Compacted<'a>, Undecidable> {
        compact_node_reference(no_vocabulary(), id, None, self.context(), Some(key), self.options)
            .map(|compacted| read_processor_json(compacted, &mut ProcessorCompacted))
            .map_err(confused)
    }

    /// The container mapping of the term `key`.
    pub(crate) fn container(&self, key: &str) -> Container {
        self.context().get(key).map_or(Container::None, |definition| definition.container())
    }

    /// Whether the values written under `key` stay an array even when there
    /// is only one.
    pub(crate) fn values_as_array(&self, key: &str) -> bool {
        values_as_array(self.container(key), key, self.options.compact_arrays)
    }

    /// Whether the `@type` values written under `type_alias` stay an array
    /// even when there is only one.
    pub(crate) fn types_as_array(&self, type_alias: &str) -> bool {
        types_as_array(self.context(), type_alias, self.options)
    }

    /// Whether a single-element array is written as its element.
    pub(crate) const fn compacts_arrays(&self) -> bool {
        self.options.compact_arrays
    }

    /// Refuse a selected property term whose definition makes compaction
    /// depend on more than the term.
    fn admit_property_term(&self, key: &str) -> Result<(), Undecidable> {
        let Some(definition) = self.context().get(key) else {
            return Ok(());
        };
        if definition.context().is_some() {
            return Err(Undecidable::PropertyScopedContext);
        }
        if definition.nest().is_some() {
            return Err(Undecidable::NestedTerm);
        }
        let container = definition.container();
        if MAP_CONTAINERS.into_iter().any(|kind| container.contains(kind)) {
            return Err(Undecidable::MapContainer);
        }
        Ok(())
    }
}

/// The walk's reading of an IRI the library cannot write.
const fn confused(_: IriConfusedWithPrefix) -> Undecidable {
    Undecidable::ConfusedIri
}

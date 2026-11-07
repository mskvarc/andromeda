//! How a compact node written under one context is read: without the
//! containers of the context's terms, each member under the core term its
//! name stands for.
//!
//! A document compacted under a context (a compacted response, a peer's
//! answer, a notification) writes a core term under the term the context
//! defines for it (clause 5.5.7, JSON-LD 1.1 API section 6.2.2) and a member
//! in the shape the container of its term gives (JSON-LD 1.1 section 4.6).
//! The containers are undone first, since a container belongs to the term as
//! written, and the aliases are read after.

use crate::{
    compact_node::{aliased_node::AliasedNode, contained_node::ContainedNode},
    container_terms::ContainerTerms,
    context_terms::ContextTerms,
    core_term_aliases::CoreTermAliases,
};

/// The terms of one context a compact node written under it is read through.
#[derive(Debug, Clone, Copy)]
pub struct ContextReading<'c> {
    /// The terms the context defines for core terms.
    aliases: &'c CoreTermAliases,
    /// The terms whose container reshapes what is compacted under them.
    containers: &'c ContainerTerms,
}

impl<'c> ContextReading<'c> {
    /// Read through `aliases` and without `containers`.
    #[must_use]
    pub const fn new(aliases: &'c CoreTermAliases, containers: &'c ContainerTerms) -> ContextReading<'c> {
        ContextReading { aliases, containers }
    }

    /// The terms the context defines for core terms.
    #[must_use]
    pub const fn aliases(self) -> &'c CoreTermAliases {
        self.aliases
    }

    /// The terms whose container reshapes what is written under them.
    #[must_use]
    pub const fn containers(self) -> &'c ContainerTerms {
        self.containers
    }

    /// `node`, read without the containers and through the aliases.
    #[must_use]
    pub const fn read<N>(self, node: N) -> AliasedNode<'c, ContainedNode<'c, N>> {
        AliasedNode::new(ContainedNode::new(node, self.containers), self.aliases)
    }
}

impl ContextReading<'static> {
    /// The reading every context gives: the clause 4.4 keyword spellings of
    /// `id` and `type`, and no container.
    #[must_use]
    pub fn keyword_spellings() -> ContextReading<'static> {
        ContextTerms::keyword_spellings().reading()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        compact_node::{CompactNode, NodeShape, context_reading::ContextReading},
        container_terms::{ContainerTerms, TermContainer},
        core_term_aliases::CoreTermAliases,
    };
    use andromeda_json_literal::piece::JsonPiece;
    use serde_json::value::RawValue;

    /// A `@set` alias of `@type` is read as `type`, its array of one as the
    /// class it holds.
    #[test]
    fn a_set_alias_of_type_reads_as_one_core_type() {
        let mut aliases = CoreTermAliases::none();
        aliases.alias("kind", "type");
        let mut containers = ContainerTerms::none();
        containers.contain("kind", TermContainer::Set);
        let text = RawValue::from_string(r#"{"kind": ["Property"]}"#.to_owned()).expect("JSON");
        let node = ContextReading::new(&aliases, &containers).read(JsonPiece::new(&text));
        let kind = node.member("type").map(CompactNode::shape);
        assert!(matches!(&kind, Some(NodeShape::String(text)) if text == "Property"), "{kind:?}");
    }

    #[test]
    fn the_keyword_spellings_read_at_type_as_type() {
        let text = RawValue::from_string(r#"{"@type": "Property"}"#.to_owned()).expect("JSON");
        let node = ContextReading::keyword_spellings().read(JsonPiece::new(&text));
        assert!(node.member("type").is_some());
    }
}

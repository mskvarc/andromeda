//! The terms of one processed context that decide how a document written
//! under it is read, held for as long as the context is.
//!
//! [`ContextReading`] borrows the aliases and the containers of a context for
//! one reading. A request context is admitted once and read from many places
//! (the body, each batch item, a Temporal Evolution), so the admission keeps
//! them together as [`ContextTerms`] and lends a [`ContextReading`] to each
//! reader.

use crate::{compact_node::context_reading::ContextReading, container_terms::ContainerTerms, core_term_aliases::CoreTermAliases};
use std::sync::LazyLock;

/// The terms every context has: the clause 4.4 keyword spellings of `id` and
/// `type`, and no container.
static KEYWORD_SPELLINGS: LazyLock<ContextTerms> = LazyLock::new(|| ContextTerms::new(CoreTermAliases::keyword_spellings().clone(), ContainerTerms::none())); // clone: once, into the terms every core-only reading shares

/// The terms a context defines for core terms and the terms whose container
/// reshapes what is written under them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextTerms {
    /// The terms the context defines for core terms.
    aliases: CoreTermAliases,
    /// The terms whose container reshapes what is written under them.
    containers: ContainerTerms,
}

impl ContextTerms {
    /// The terms every context has: the clause 4.4 keyword spellings of `id`
    /// and `type`, and no container.
    #[must_use]
    pub fn keyword_spellings() -> &'static ContextTerms {
        &KEYWORD_SPELLINGS
    }

    /// The terms `aliases` and `containers` make up.
    #[must_use]
    pub const fn new(aliases: CoreTermAliases, containers: ContainerTerms) -> ContextTerms {
        ContextTerms { aliases, containers }
    }

    /// The terms the context defines for core terms.
    #[must_use]
    pub const fn aliases(&self) -> &CoreTermAliases {
        &self.aliases
    }

    /// The terms whose container reshapes what is written under them.
    #[must_use]
    pub const fn containers(&self) -> &ContainerTerms {
        &self.containers
    }

    /// How a document written under the context is read.
    #[must_use]
    pub const fn reading(&self) -> ContextReading<'_> {
        ContextReading::new(&self.aliases, &self.containers)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        container_terms::{ContainerTerms, TermContainer},
        context_terms::ContextTerms,
        core_term_aliases::CoreTermAliases,
    };
    use std::borrow::Cow;

    #[test]
    fn the_terms_of_a_context_are_the_aliases_and_containers_it_was_built_from() {
        let mut aliases = CoreTermAliases::none();
        aliases.alias("kind", "type");
        let mut containers = ContainerTerms::none();
        containers.contain("kind", TermContainer::Set);
        let terms = ContextTerms::new(aliases, containers);
        pretty_assertions::assert_eq!(terms.aliases().core_name(Cow::Borrowed("kind")), "type");
        pretty_assertions::assert_eq!(terms.containers().container_of("kind"), Some(TermContainer::Set));
    }
}

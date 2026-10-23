//! The one rule for reading a core NGSI-LD term written under another term.
//!
//! Clause 4.4 lets a document write `@id` and `@type` where `id` and `type`
//! stand. Clause 5.5.7 reads a request under its user `@context`, which may
//! define a term of its own for a core term: an alias of `@type`, a second
//! term for `ngsi-ld:hasValue`. Compaction selects such a term for a response
//! too (JSON-LD 1.1 API section 6.2.2, clause 4.5.0). The data is the same
//! whichever term spells it, so every reader of a compact document reads a
//! member written under an alias as the core term it stands for, and a `type`
//! value naming a core class through an alias as that class.

use andromeda_ngsi_ld_vocabulary::keyword;
use std::{borrow::Cow, collections::HashMap, sync::LazyLock};

/// The clause 4.4 keyword spellings of `id` and `type`, read under any
/// context.
static KEYWORD_SPELLINGS: LazyLock<CoreTermAliases> = LazyLock::new(|| {
    let mut aliases = CoreTermAliases::none();
    aliases.alias(keyword::ID, keyword::ID_ALIAS);
    aliases.alias(keyword::TYPE, keyword::TYPE_ALIAS);
    aliases
});

/// Where a value stands in a compact document, which decides whether its
/// strings are read through the aliases.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Standing {
    /// Any value: only the names of its members are read through the aliases.
    #[default]
    Node,
    /// The value of a `type` member: its strings name classes, read through
    /// the aliases too.
    Types,
}

impl Standing {
    /// Where the value of the member whose core name is `name` stands.
    #[must_use]
    pub fn of_member(name: &str) -> Standing {
        if name == keyword::TYPE_ALIAS { Standing::Types } else { Standing::Node }
    }
}

/// The terms a context defines for core NGSI-LD terms, each mapped to the
/// core term it stands for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoreTermAliases(HashMap<Box<str>, &'static str>);

impl CoreTermAliases {
    /// No alias: every member is read under the name it is written with.
    #[must_use]
    pub fn none() -> CoreTermAliases {
        CoreTermAliases(HashMap::new())
    }

    /// The aliases every context has: `@id` read as `id` and `@type` as
    /// `type` (clause 4.4).
    #[must_use]
    pub fn keyword_spellings() -> &'static CoreTermAliases {
        &KEYWORD_SPELLINGS
    }

    /// Read the term `alias` as the core term `core`.
    pub fn alias(&mut self, alias: &str, core: &'static str) {
        if alias != core {
            self.0.insert(Box::from(alias), core);
        }
    }

    /// Whether no term is read under another name.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The core member a member written `name` stands for: `name` itself
    /// when the context defines it for no core term.
    #[must_use]
    pub fn core_name<'n>(&self, name: Cow<'n, str>) -> Cow<'n, str> {
        self.core_term(name)
    }

    /// The core class a `type` value written `value` names: `value` itself
    /// when the context defines it for no core term. A context term for
    /// `ngsi-ld:Property` names the `Property` class, as a member written
    /// under it names nothing a member could be.
    #[must_use]
    pub fn core_class<'n>(&self, value: Cow<'n, str>) -> Cow<'n, str> {
        self.core_term(value)
    }

    /// The string `text`, standing where `standing` says, read through the
    /// aliases: a class name of a `type` member is read as the core class it
    /// names, any other string as written.
    #[must_use]
    pub fn string<'n>(&self, text: Cow<'n, str>, standing: Standing) -> Cow<'n, str> {
        match standing {
            Standing::Types => self.core_class(text),
            Standing::Node => text,
        }
    }

    /// The core term `written` stands for, or `written` itself.
    fn core_term<'n>(&self, written: Cow<'n, str>) -> Cow<'n, str> {
        match self.0.get(written.as_ref()).copied() {
            Some(core) => Cow::Borrowed(core),
            None => written,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::core_term_aliases::{CoreTermAliases, Standing};
    use std::borrow::Cow;

    #[test]
    fn keyword_spellings_name_the_core_terms() {
        let aliases = CoreTermAliases::keyword_spellings();
        pretty_assertions::assert_eq!(aliases.core_name(Cow::Borrowed("@id")), "id");
        pretty_assertions::assert_eq!(aliases.core_name(Cow::Borrowed("@type")), "type");
        pretty_assertions::assert_eq!(aliases.core_name(Cow::Borrowed("@value")), "@value");
        pretty_assertions::assert_eq!(aliases.core_name(Cow::Borrowed("speed")), "speed");
    }

    #[test]
    fn a_type_value_naming_a_core_class_alias_reads_as_the_class() {
        let mut aliases = CoreTermAliases::none();
        aliases.alias("Prop", "Property");
        pretty_assertions::assert_eq!(aliases.core_class(Cow::Borrowed("Prop")), "Property");
        pretty_assertions::assert_eq!(aliases.core_class(Cow::Borrowed("Vehicle")), "Vehicle");
    }

    #[test]
    fn only_a_string_of_a_type_member_is_read_as_a_class() {
        let mut aliases = CoreTermAliases::none();
        aliases.alias("Prop", "Property");
        pretty_assertions::assert_eq!(Standing::of_member("type"), Standing::Types);
        pretty_assertions::assert_eq!(Standing::of_member("value"), Standing::Node);
        pretty_assertions::assert_eq!(aliases.string(Cow::Borrowed("Prop"), Standing::Types), "Property");
        pretty_assertions::assert_eq!(aliases.string(Cow::Borrowed("Prop"), Standing::Node), "Prop");
    }

    #[test]
    fn a_term_aliasing_itself_is_no_alias() {
        let mut aliases = CoreTermAliases::none();
        aliases.alias("value", "value");
        assert!(aliases.is_empty());
    }
}

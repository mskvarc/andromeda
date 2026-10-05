//! The terms a request context defines for the core NGSI-LD terms.
//!
//! Compaction selects, for every reserved member and every core class, the
//! term the request context prefers (JSON-LD 1.1 API section 6.2.2), which
//! need not be the core term: a context may alias `@type`, or define a shorter
//! term for `ngsi-ld:hasValue`. The compact model reads core terms, so every
//! term of the context that stands for a core term is read as that term.

use andromeda_ngsi_ld_representation::compact_node::aliased_node::CoreTermAliases;
use andromeda_ngsi_ld_vocabulary::expanded_to_compact;
use jsonld::{BlankIdBuf, Context, IriBuf, Term, context::BindingRef, syntax::Keyword};

/// The core term an entity's identifier is read under.
const ID_MEMBER: &str = "id";

/// The core term an entity's or an instance's types are read under.
const TYPE_MEMBER: &str = "type";

/// Every term `context` defines for a core term, mapped to that core term.
pub(crate) fn core_term_aliases(context: &Context<IriBuf, BlankIdBuf>) -> CoreTermAliases {
    let mut aliases = CoreTermAliases::none();
    // The keywords themselves stand for the core aliases of clause 4.4 under
    // any context, the processor writing a keyword it found no term for.
    aliases.alias(Keyword::Id.into_str(), ID_MEMBER);
    aliases.alias(Keyword::Type.into_str(), TYPE_MEMBER);
    for binding in context.definitions() {
        let (term, definition) = match binding {
            BindingRef::Normal(term, definition) => (term.as_str(), definition),
            BindingRef::Type(_) => continue,
        };
        let core = match definition.value() {
            Some(Term::Keyword(Keyword::Id)) => Some(ID_MEMBER),
            Some(Term::Keyword(Keyword::Type)) => Some(TYPE_MEMBER),
            Some(Term::Id(id)) => expanded_to_compact(id.as_str()),
            Some(Term::Keyword(_) | Term::Null) | None => None,
        };
        if let Some(core) = core {
            aliases.alias(term, core);
        }
    }
    aliases
}

#[cfg(test)]
mod tests {
    use andromeda_ngsi_ld_vocabulary::expanded_to_compact;

    /// Every value carrier names its core term, which an alias is read as.
    #[test]
    fn every_value_carrier_resolves_to_its_core_term() {
        for (iri, core) in [
            ("https://uri.etsi.org/ngsi-ld/hasValue", "value"),
            ("https://uri.etsi.org/ngsi-ld/hasObject", "object"),
            ("https://uri.etsi.org/ngsi-ld/hasJSON", "json"),
            ("https://uri.etsi.org/ngsi-ld/hasVocab", "vocab"),
            ("https://uri.etsi.org/ngsi-ld/hasLanguageMap", "languageMap"),
            ("https://uri.etsi.org/ngsi-ld/hasValueList", "valueList"),
            ("https://uri.etsi.org/ngsi-ld/hasObjectList", "objectList"),
        ] {
            pretty_assertions::assert_eq!(expanded_to_compact(iri), Some(core), "{iri}");
        }
    }
}

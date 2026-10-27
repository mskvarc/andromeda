//! Which physical relation holds the dictionary of one interned name type.

use andromeda_ngsi_ld_lexicon::reference::NgsiLdReferenceError;
use andromeda_postgresql_query::dictionary::interned_name::InternedName;
use andromeda_telemetry::name_dictionary::NameDictionary;
use sea_query::DynIden;
use std::str::FromStr;
use strum::{Display, IntoStaticStr};

/// The two dictionaries a tenant database carries.
///
/// Named so an error, a metric or a log line can say which dictionary it is
/// about without carrying the relation's iden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum DictionaryKind {
    /// The `attribute_names` dictionary of expanded attribute IRIs.
    AttributeNames,
    /// The `entity_type_names` dictionary of expanded entity type IRIs.
    EntityTypeNames,
}

// No derive maps one enum's variants onto another crate's enum.
impl From<DictionaryKind> for NameDictionary {
    fn from(kind: DictionaryKind) -> NameDictionary {
        match kind {
            DictionaryKind::AttributeNames => NameDictionary::AttributeNames,
            DictionaryKind::EntityTypeNames => NameDictionary::EntityTypeNames,
        }
    }
}

/// An interned name type together with the relation that interns it.
///
/// [`InternedName`] says which name types carry a dictionary key and what
/// that key is; this trait says where the rows live, so one cache and one
/// port implementation serve every dictionary. Every stored row is parsed
/// back into the name type, and both name types share the clause 4.6.2 grammar
/// error, which is what the [`FromStr`] bound pins.
pub trait DictionaryRelation: InternedName + FromStr<Err = NgsiLdReferenceError> {
    /// Which dictionary this is.
    const KIND: DictionaryKind;

    /// The dictionary table.
    fn table() -> DynIden;

    /// The `INTEGER` key column.
    fn key_column() -> DynIden;

    /// The name text column.
    fn name_column() -> DynIden;
}

#[cfg(test)]
mod tests {
    use crate::dictionary_relation::DictionaryKind;
    use andromeda_telemetry::name_dictionary::NameDictionary;
    use pretty_assertions::assert_eq;

    #[test]
    fn dictionary_kinds_render_in_kebab_case() {
        assert_eq!(DictionaryKind::AttributeNames.to_string(), "attribute-names");
        assert_eq!(DictionaryKind::EntityTypeNames.to_string(), "entity-type-names");
    }

    #[test]
    fn each_dictionary_kind_names_its_own_metric_dictionary() {
        assert_eq!(NameDictionary::from(DictionaryKind::AttributeNames), NameDictionary::AttributeNames);
        assert_eq!(NameDictionary::from(DictionaryKind::EntityTypeNames), NameDictionary::EntityTypeNames);
    }
}

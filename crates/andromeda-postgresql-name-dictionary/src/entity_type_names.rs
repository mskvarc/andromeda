//! The `entity_type_names` dictionary of expanded entity type IRIs.

use crate::dictionary_relation::{DictionaryKind, DictionaryRelation};
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_postgresql_schema::name_dictionary::EntityTypeNames;
use sea_query::{DynIden, IntoIden};

impl DictionaryRelation for EntityTypeName {
    const KIND: DictionaryKind = DictionaryKind::EntityTypeNames;

    fn table() -> DynIden {
        EntityTypeNames::Table.into_iden()
    }

    fn id_column() -> DynIden {
        EntityTypeNames::Id.into_iden()
    }

    fn name_column() -> DynIden {
        EntityTypeNames::Name.into_iden()
    }
}

#[cfg(test)]
mod tests {
    use crate::dictionary_relation::{DictionaryKind, DictionaryRelation};
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use pretty_assertions::assert_eq;

    #[test]
    fn entity_type_names_are_interned_in_the_entity_type_names_relation() {
        assert_eq!(<EntityTypeName as DictionaryRelation>::KIND, DictionaryKind::EntityTypeNames);
        assert_eq!(<EntityTypeName as DictionaryRelation>::table().to_string(), "entity_type_names");
        assert_eq!(<EntityTypeName as DictionaryRelation>::id_column().to_string(), "id");
        assert_eq!(<EntityTypeName as DictionaryRelation>::name_column().to_string(), "name");
    }
}

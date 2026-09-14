//! The `attribute_names` dictionary of expanded attribute IRIs.

use crate::dictionary_relation::{DictionaryKind, DictionaryRelation};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_postgresql_schema::name_dictionary::AttributeNames;
use sea_query::{DynIden, IntoIden};

impl DictionaryRelation for AttributeName {
    const KIND: DictionaryKind = DictionaryKind::AttributeNames;

    fn table() -> DynIden {
        AttributeNames::Table.into_iden()
    }

    fn id_column() -> DynIden {
        AttributeNames::Id.into_iden()
    }

    fn name_column() -> DynIden {
        AttributeNames::Name.into_iden()
    }
}

#[cfg(test)]
mod tests {
    use crate::dictionary_relation::{DictionaryKind, DictionaryRelation};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use pretty_assertions::assert_eq;

    #[test]
    fn attribute_names_are_interned_in_the_attribute_names_relation() {
        assert_eq!(<AttributeName as DictionaryRelation>::KIND, DictionaryKind::AttributeNames);
        assert_eq!(<AttributeName as DictionaryRelation>::table().to_string(), "attribute_names");
        assert_eq!(<AttributeName as DictionaryRelation>::id_column().to_string(), "id");
        assert_eq!(<AttributeName as DictionaryRelation>::name_column().to_string(), "name");
    }
}

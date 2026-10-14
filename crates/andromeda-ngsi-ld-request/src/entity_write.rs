//! The parameters of the entity writes that name their target by `type`.

use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use serde::Deserialize;

/// Query parameters for `DELETE /entities/{id}` (Delete Entity, Table
/// 6.5.3.2-1) and `PUT /entities/{id}` (Replace Entity, Table 6.5.3.3-1).
///
/// Create Entity takes neither table: clause 6.4.3.1 defines no URL parameter
/// for it, so it keeps [`crate::local::LocalOnlyParams`].
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityWriteParams {
    /// The clause 4.17 selector the target entity's types must satisfy.
    #[serde(rename = "type")]
    pub entity_type: Option<TypeExprRaw>,
    /// Whether the operation is confined to this broker (clause 5.5.13).
    pub local: Option<bool>,
}

/// Query parameters for `PATCH /entities/{id}/attrs` (Update Attributes, Table
/// 6.6.3.2-1).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateAttributesParams {
    /// The clause 4.17 selector the target entity's types must satisfy.
    #[serde(rename = "type")]
    pub entity_type: Option<TypeExprRaw>,
}

#[cfg(test)]
mod tests {
    use crate::entity_write::{EntityWriteParams, UpdateAttributesParams};

    /// Tables 6.5.3.2-1 and 6.5.3.3-1: `type` is read beside `local`, and a
    /// malformed selector is a client error.
    #[test]
    fn an_entity_write_reads_a_type_selector_beside_the_local_flag() {
        let params: EntityWriteParams = serde_urlencoded::from_str("type=Vehicle&local=true").expect("both parameters parse");
        pretty_assertions::assert_eq!(params.entity_type.map(|selector| selector.to_string()), Some("Vehicle".to_string()));
        pretty_assertions::assert_eq!(params.local, Some(true));
        let bare: EntityWriteParams = serde_urlencoded::from_str("").expect("parses");
        assert!(bare.entity_type.is_none() && bare.local.is_none());
        assert!(serde_urlencoded::from_str::<EntityWriteParams>("type=Vehicle%7C").is_err());
        assert!(serde_urlencoded::from_str::<EntityWriteParams>("unknown=x").is_err());
    }

    /// Table 6.6.3.2-1 defines `type` alone.
    #[test]
    fn an_update_attributes_reads_a_type_selector_and_nothing_else() {
        let params: UpdateAttributesParams = serde_urlencoded::from_str("type=Vehicle%7CBuilding").expect("a valid type selector");
        pretty_assertions::assert_eq!(params.entity_type.map(|selector| selector.to_string()), Some("Vehicle,Building".to_string()));
        let bare: UpdateAttributesParams = serde_urlencoded::from_str("").expect("parses");
        assert!(bare.entity_type.is_none());
        assert!(serde_urlencoded::from_str::<UpdateAttributesParams>("local=true").is_err());
    }
}

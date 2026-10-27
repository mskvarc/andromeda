//! The parameters of the entity writes that name their target by `type`.

use crate::{
    query_parameter::{QueryParameter, QueryParameterSet},
    query_parameter_set,
};
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

query_parameter_set!(EntityWriteParams { Type, Local });

/// Query parameters for `PATCH /entities/{id}/attrs` (Update Attributes, Table
/// 6.6.3.2-1).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateAttributesParams {
    /// The clause 4.17 selector the target entity's types must satisfy.
    #[serde(rename = "type")]
    pub entity_type: Option<TypeExprRaw>,
    /// Whether the operation is confined to this broker (clause 6.3.18 `local`).
    pub local: Option<bool>,
}

query_parameter_set!(UpdateAttributesParams { Type, Local });

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
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

    /// Clause 6.3.18 gives Update Attributes `local` beside its `type`.
    #[test]
    fn an_attribute_update_reads_the_local_flag() {
        let params: UpdateAttributesParams = serde_urlencoded::from_str("type=Vehicle&local=true").expect("both parameters parse");
        pretty_assertions::assert_eq!(params.local, Some(true));
    }

    /// Table 6.6.3.2-1 defines `type` and clause 6.3.18 adds `local`; nothing
    /// else is a parameter of the operation.
    #[test]
    fn an_update_attributes_reads_a_type_selector_and_nothing_else() {
        let params: UpdateAttributesParams = serde_urlencoded::from_str("type=Vehicle%7CBuilding").expect("a valid type selector");
        pretty_assertions::assert_eq!(params.entity_type.map(|selector| selector.to_string()), Some("Vehicle,Building".to_string()));
        let bare: UpdateAttributesParams = serde_urlencoded::from_str("").expect("parses");
        assert!(bare.entity_type.is_none() && bare.local.is_none());
        assert!(serde_urlencoded::from_str::<UpdateAttributesParams>("options=noOverwrite").is_err());
    }
}

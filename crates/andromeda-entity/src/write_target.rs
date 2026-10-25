//! The entity a write operation targets.
//!
//! Clauses 5.6.2.4 to 5.6.6.4, 5.6.17.4, 5.6.18.4 and 5.6.19.4 know the target
//! of a write as the entity "whose id (URI), and where specified type, is
//! equivalent": an entity outside the clause 4.17 selector the request carried
//! in its `type` parameter is not the target, and the operation answers
//! `ResourceNotFound`. The identifier and the selector are therefore one value,
//! so no write can name one without the other.

use crate::{
    EntityService,
    entity_type_match::{entity_satisfies, types_satisfy},
    error::EntityError,
};
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_query::type_selection::{
    expansion::expand_type_expression,
    grammar::{TypeExpr, TypeExprRaw},
};

/// The entity a write targets: its identifier and, where the request specified
/// one, the expanded clause 4.17 selector its types must satisfy.
#[derive(Debug, Clone, Copy)]
pub struct WriteTarget<'a> {
    /// The identifier of the targeted entity.
    pub id: &'a Identifier,
    /// The selector the targeted entity's types must satisfy, when the request
    /// carried a `type` parameter.
    pub entity_type: Option<&'a TypeExpr>,
}

impl<'a> WriteTarget<'a> {
    /// The entity `id`, of any type when `entity_type` is `None`.
    #[must_use]
    pub const fn new(id: &'a Identifier, entity_type: Option<&'a TypeExpr>) -> WriteTarget<'a> {
        WriteTarget { id, entity_type }
    }

    /// Hold `state`, the stored entity this write was computed against, to the
    /// selector.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError::NotFound`] when the selector excludes the entity.
    pub(crate) fn admits(&self, state: &EntityState) -> Result<(), EntityError> {
        match self.entity_type {
            Some(selector) if !entity_satisfies(state, selector) => Err(EntityError::not_found(self.id)),
            Some(_) | None => Ok(()),
        }
    }

    /// Hold an entity of `types`, as a deletion outlines it, to the selector.
    pub(crate) fn selects(&self, types: &[EntityTypeName]) -> bool {
        self.entity_type.is_none_or(|selector| types_satisfy(types, selector))
    }
}

impl EntityService {
    /// Expand the `type` parameter of a write request under the request
    /// context, once, for the [`WriteTarget`] every attempt of the write is held
    /// to (clause 4.17).
    ///
    /// # Errors
    ///
    /// Returns [`EntityError::JsonLd`] when a type of the selector cannot be
    /// expanded against `context`.
    pub async fn expand_write_selector(&self, selector: Option<&TypeExprRaw>, context: &ContextSpec) -> Result<Option<TypeExpr>, EntityError> {
        match selector {
            Some(raw) => {
                let prepared = self.engine.prepare_term_context(context);
                Ok(Some(expand_type_expression(raw, &ContextualExpander::new(&self.engine, &prepared)).await?))
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{error::EntityError, write_target::WriteTarget};
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::type_selection::grammar::TypeExpr;
    use iri_rs::IriBuf;
    use serde_json::json;

    fn vehicle() -> EntityState {
        let document = json!([{"@id": "urn:ngsi-ld:Vehicle:1", "@type": ["https://example.org/Vehicle"]}]);
        EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored state")
    }

    fn named(name: &str) -> TypeExpr {
        TypeExpr::Type(IriBuf::new(format!("https://example.org/{name}")).expect("valid IRI"))
    }

    fn id() -> Identifier {
        "urn:ngsi-ld:Vehicle:1".parse().expect("a valid identifier")
    }

    /// Clauses 5.6.2.4 to 5.6.19.4: no selector names an entity of any type.
    #[test]
    fn a_target_without_a_selector_admits_any_entity() {
        let id = id();
        WriteTarget::new(&id, None).admits(&vehicle()).expect("no selector holds the entity to nothing");
    }

    /// Clause 5.6.2.4: an entity outside the selector is not the target.
    #[test]
    fn a_target_refuses_an_entity_outside_its_selector() {
        let id = id();
        let building = named("Building");
        let error = WriteTarget::new(&id, Some(&building)).admits(&vehicle()).expect_err("outside the selector");
        assert!(matches!(error, EntityError::NotFound { .. }), "{error:?}");
        let selector = TypeExpr::Or(vec![named("Building"), named("Vehicle")]);
        WriteTarget::new(&id, Some(&selector)).admits(&vehicle()).expect("inside the selector");
    }
}

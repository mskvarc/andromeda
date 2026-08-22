//! A fixed-answer [`RegisteredEntityProbe`] for policy unit tests.

use crate::{
    entity_probe::{EntityPresence, RegisteredEntityProbe},
    error::RegistrationError,
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::entity_info::EntityInfo;
use futures::future::BoxFuture;
use iri_rs::IriBuf;
use std::sync::Mutex;

/// A probe answering a fixed presence and recording what it was asked.
pub(crate) struct FixedProbe {
    holds: EntityPresence,
    matches: EntityPresence,
    asked: Mutex<Vec<String>>,
}

impl FixedProbe {
    /// A probe answering `holds` to `holds_any_attribute` and `matches` to
    /// `any_entity_matching`.
    pub(crate) fn new(holds: EntityPresence, matches: EntityPresence) -> FixedProbe {
        FixedProbe {
            holds,
            matches,
            asked: Mutex::new(Vec::new()),
        }
    }

    /// Every question the probe was asked, in order.
    pub(crate) fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("probe log").clone() // clone: the test reads what the fake recorded
    }
}

impl RegisteredEntityProbe for FixedProbe {
    fn holds_any_attribute<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        entity_id: &'life1 Uri,
        _attributes: &'life2 [IriBuf],
    ) -> BoxFuture<'async_trait, Result<EntityPresence, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
    {
        self.asked.lock().expect("probe log").push(format!("holds:{entity_id}"));
        Box::pin(async { Ok(self.holds) })
    }

    fn any_entity_matching<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        selector: Option<&'life1 EntityInfo>,
        _attributes: &'life2 [IriBuf],
    ) -> BoxFuture<'async_trait, Result<EntityPresence, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
    {
        let description = selector.map_or_else(
            || "matches:<none>".to_string(),
            |selector| format!("matches:{}", selector.type_.as_type_expression()),
        );
        self.asked.lock().expect("probe log").push(description);
        Box::pin(async { Ok(self.matches) })
    }
}

//! The context-source-registration change event CSR subscriptions match on
//! (ETSI GS CIM 009 clause 5.3.2).

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::notification::CsrTriggerReason;
use serde_json::Value;

/// A CSR mutation that may trigger CSR subscription notifications.
#[derive(Clone, Debug)]
pub struct CsrChangeEvent {
    /// Identifier of the mutated registration.
    pub csr_id: Uri,
    /// What kind of mutation this was.
    pub trigger_reason: CsrTriggerReason,
    /// For delete events, a snapshot of the CSR before deletion.
    pub csr_snapshot: Option<Value>,
}

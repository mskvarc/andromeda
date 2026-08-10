//! Focused immutable component-report route state.

use andromeda_component_report::ComponentReport;
use std::sync::Arc;

/// State containing exactly the component reports selected by composition.
#[derive(Clone)]
pub struct ComponentReportState {
    /// Immutable active-component inventory.
    components: Arc<[ComponentReport]>,
}

impl ComponentReportState {
    /// Construct focused state from the active component inventory.
    pub fn new(components: Vec<ComponentReport>) -> ComponentReportState {
        ComponentReportState { components: components.into() }
    }

    /// Borrow every active component in composition order.
    pub fn components(&self) -> &[ComponentReport] {
        &self.components
    }
}

//! The routing switches the resolved extension set decides.
//!
//! An HTTP route crate may not depend on the extension set (the HTTP stack
//! depends on it, so the reverse would be a cycle), so each switch it gates on
//! is a type of its own, derived here from the one resolved set.

use andromeda_extensions::enabled::EnabledExtensions;
use andromeda_http_extraction::accept_language::AcceptLanguageReading;
use andromeda_http_tenancy::routes::TenantManagement;

/// Whether the operations that take `lang` read `Accept-Language`.
pub(crate) fn accept_language_reading(enabled: EnabledExtensions) -> AcceptLanguageReading {
    if enabled.accept_language_enabled() {
        AcceptLanguageReading::Honoured
    } else {
        AcceptLanguageReading::Ignored
    }
}

/// The tenant-management routing switch the HTTP crate gates on.
pub(crate) fn tenant_management_mode(enabled: EnabledExtensions) -> TenantManagement {
    if enabled.tenant_management_enabled() {
        TenantManagement::Enabled
    } else {
        TenantManagement::Disabled
    }
}

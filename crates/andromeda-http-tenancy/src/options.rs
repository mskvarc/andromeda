//! The `options` keywords a tenant read accepts.

use andromeda_http_response::classified_report::ClassifiedReport;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;

/// Which optional blocks a tenant read includes.
///
/// Both members are genuine present-or-absent switches: `sysAttrs` surfaces the
/// system timestamps (clause 6.3.11) and `usage` the operational block. Both
/// default off, so a plain read is cheap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TenantOptions {
    /// Whether to surface the top-level `createdAt`/`modifiedAt` timestamps.
    pub sys_attrs: bool,
    /// Whether to gather and include the operational usage block.
    pub usage: bool,
}

/// An `options` value naming a keyword the tenant surface does not define.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("unsupported options keyword; the tenant surface accepts 'sysAttrs' and 'usage'")]
pub struct UnsupportedOption;

impl UnsupportedOption {
    /// The clause 6.3.20 `InvalidRequest` this becomes as a classified report.
    #[must_use]
    pub fn classify(&self) -> ClassifiedReport {
        ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, self.to_string())
    }
}

impl TenantOptions {
    /// Parse a comma-separated `options` value into the selected blocks.
    ///
    /// Absent or empty selects nothing. An unrecognised keyword is refused rather
    /// than ignored (clause 6.3.20), so a client cannot silently mistype `usage`
    /// and receive a cheap read.
    ///
    /// # Errors
    ///
    /// Returns [`UnsupportedOption`] when any keyword lies outside
    /// `{sysAttrs, usage}`.
    pub fn parse(written: Option<&str>) -> Result<TenantOptions, UnsupportedOption> {
        let mut options = TenantOptions::default();
        let Some(written) = written else {
            return Ok(options);
        };
        for keyword in written.split(',').map(str::trim).filter(|keyword| !keyword.is_empty()) {
            match keyword {
                "sysAttrs" => options.sys_attrs = true,
                "usage" => options.usage = true,
                _unsupported => return Err(UnsupportedOption),
            }
        }
        Ok(options)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::options::{TenantOptions, UnsupportedOption};

    #[test]
    fn an_absent_or_empty_value_selects_nothing() {
        pretty_assertions::assert_eq!(TenantOptions::parse(None).expect("parses"), TenantOptions::default());
        pretty_assertions::assert_eq!(TenantOptions::parse(Some("")).expect("parses"), TenantOptions::default());
    }

    #[test]
    fn each_keyword_and_their_combination_are_accepted() {
        pretty_assertions::assert_eq!(
            TenantOptions::parse(Some("sysAttrs")).expect("parses"),
            TenantOptions { sys_attrs: true, usage: false }
        );
        pretty_assertions::assert_eq!(
            TenantOptions::parse(Some("usage")).expect("parses"),
            TenantOptions { sys_attrs: false, usage: true }
        );
        pretty_assertions::assert_eq!(
            TenantOptions::parse(Some("sysAttrs,usage")).expect("parses"),
            TenantOptions { sys_attrs: true, usage: true }
        );
    }

    #[test]
    fn an_unknown_keyword_is_refused() {
        pretty_assertions::assert_eq!(TenantOptions::parse(Some("sysAttrs,bogus")), Err(UnsupportedOption));
    }
}

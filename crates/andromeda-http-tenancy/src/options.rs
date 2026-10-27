//! The `options` keywords a tenant read accepts.

use andromeda_http_response::classified_report::ClassifiedReport;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_query::comma_separated_list::{CommaSeparatedListError, ListedItem, listed_members};

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

/// A keyword the tenant surface defines for `options`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumString)]
pub enum TenantOption {
    /// The system timestamps (clause 6.3.11).
    #[strum(serialize = "sysAttrs")]
    SysAttrs,
    /// The operational usage block.
    #[strum(serialize = "usage")]
    Usage,
}

/// An `options` value the tenant surface cannot act on.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum TenantOptionsRefusal {
    /// The list is empty or holds an empty keyword.
    #[error(transparent)]
    EmptyKeyword(#[from] CommaSeparatedListError),
    /// A keyword the tenant surface does not define.
    #[error("unsupported options keyword; the tenant surface accepts 'sysAttrs' and 'usage'")]
    Unsupported,
    /// A keyword named twice.
    #[error("duplicate options keyword '{0}'")]
    Duplicate(TenantOption),
}

impl TenantOptionsRefusal {
    /// The clause 6.3.20 `InvalidRequest` this becomes as a classified report.
    #[must_use]
    pub fn classify(&self) -> ClassifiedReport {
        ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, self.to_string())
    }
}

impl TenantOptions {
    /// Parse a comma-separated `options` value into the selected blocks.
    ///
    /// Absent selects nothing. A malformed list, an unrecognised keyword and a
    /// repeated one are refused rather than ignored (clause 6.3.20), as for every
    /// other `options` parameter, so a client cannot silently mistype `usage`
    /// and receive a cheap read.
    ///
    /// # Errors
    ///
    /// Returns [`TenantOptionsRefusal::EmptyKeyword`] for an empty list or an
    /// empty keyword, [`TenantOptionsRefusal::Unsupported`] for a keyword
    /// outside `{sysAttrs, usage}` and [`TenantOptionsRefusal::Duplicate`] for
    /// a keyword named twice.
    pub fn parse(written: Option<&str>) -> Result<TenantOptions, TenantOptionsRefusal> {
        let mut options = TenantOptions::default();
        let Some(written) = written else {
            return Ok(options);
        };
        for keyword in listed_members(written, ListedItem::OptionsToken) {
            let keyword = keyword?.parse().map_err(|_| TenantOptionsRefusal::Unsupported)?;
            let selected = match keyword {
                TenantOption::SysAttrs => &mut options.sys_attrs,
                TenantOption::Usage => &mut options.usage,
            };
            if *selected {
                return Err(TenantOptionsRefusal::Duplicate(keyword));
            }
            *selected = true;
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
    use crate::options::{TenantOption, TenantOptions, TenantOptionsRefusal};
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_ngsi_ld_query::comma_separated_list::{CommaSeparatedListError, ListedItem};

    #[test]
    fn an_absent_value_selects_nothing() {
        pretty_assertions::assert_eq!(TenantOptions::parse(None).expect("parses"), TenantOptions::default());
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
            TenantOptions::parse(Some("sysAttrs, usage")).expect("parses"),
            TenantOptions { sys_attrs: true, usage: true }
        );
    }

    #[test]
    fn an_unknown_keyword_is_refused() {
        pretty_assertions::assert_eq!(TenantOptions::parse(Some("sysAttrs,bogus")), Err(TenantOptionsRefusal::Unsupported));
    }

    #[test]
    fn an_empty_list_or_keyword_is_refused() {
        for written in ["", "sysAttrs,,usage", ",usage", "usage,", "sysAttrs, ,usage"] {
            pretty_assertions::assert_eq!(
                TenantOptions::parse(Some(written)),
                Err(TenantOptionsRefusal::EmptyKeyword(CommaSeparatedListError::EmptyMember {
                    list: ListedItem::OptionsToken
                })),
                "{written:?}"
            );
        }
    }

    #[test]
    fn a_repeated_keyword_is_refused() {
        pretty_assertions::assert_eq!(
            TenantOptions::parse(Some("usage,usage")),
            Err(TenantOptionsRefusal::Duplicate(TenantOption::Usage))
        );
        pretty_assertions::assert_eq!(
            TenantOptions::parse(Some("sysAttrs,usage,sysAttrs")),
            Err(TenantOptionsRefusal::Duplicate(TenantOption::SysAttrs))
        );
    }

    #[test]
    fn every_refusal_is_invalid_request() {
        for refusal in [
            TenantOptionsRefusal::EmptyKeyword(CommaSeparatedListError::EmptyMember {
                list: ListedItem::OptionsToken,
            }),
            TenantOptionsRefusal::Unsupported,
            TenantOptionsRefusal::Duplicate(TenantOption::Usage),
        ] {
            pretty_assertions::assert_eq!(refusal.classify().error_type(), NgsiLdErrorType::InvalidRequest);
        }
    }
}

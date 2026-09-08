//! SPDX-FileCopyrightText: 2014-2025 Sean `McArthur`
//! SPDX-License-Identifier: MIT
//!
//! Strict `Warning` response header (RFC 9110, section 5.5).
//!
//! The structure and wire-format handling are adapted from the disabled
//! `Warning` implementation in <https://github.com/hyperium/headers>. It is
//! kept locally because that implementation is not exported by the current
//! upstream crate.
//! This file, including local modifications, is distributed under the MIT License.

use http::{HeaderName, HeaderValue};
use std::{fmt, iter, str::FromStr};

/// The lowercase `Warning` field name this header is transmitted under.
static WARNING: HeaderName = HeaderName::from_static("warning");

/// One RFC 9110 warning value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// Three-digit warning code.
    pub code: u16,
    /// Warning agent or pseudonym.
    pub agent: String,
    /// Human-readable warning text, without surrounding quotes.
    pub text: String,
    /// Optional warning date.
    pub date: Option<httpdate::HttpDate>,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !(100..=999).contains(&self.code) || self.agent.is_empty() {
            return Err(fmt::Error);
        }
        write!(f, "{:03} {} \"{}\"", self.code, self.agent, self.text)?;
        if let Some(date) = self.date {
            write!(f, " \"{date}\"")?;
        }
        Ok(())
    }
}

impl FromStr for Warning {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.splitn(3, char::is_whitespace);
        let code = parts.next().ok_or(())?.parse::<u16>().map_err(|_| ())?;
        if !(100..=999).contains(&code) {
            return Err(());
        }
        let agent = parts.next().ok_or(())?.to_owned();
        let rest = parts.next().ok_or(())?.trim_start();
        let text_end = rest[1..].find('"').map(|index| index + 1).ok_or(())?;
        if !rest.starts_with('"') || text_end == 0 {
            return Err(());
        }
        let text = rest[1..text_end].to_owned();
        let tail = rest[text_end + 1..].trim();
        let date = if tail.is_empty() {
            None
        } else {
            let date_text = tail.strip_prefix('"').and_then(|s| s.strip_suffix('"')).ok_or(())?;
            Some(date_text.parse::<httpdate::HttpDate>().map_err(|_| ())?)
        };
        Ok(Self { code, agent, text, date })
    }
}

impl headers::Header for Warning {
    fn name() -> &'static HeaderName {
        &WARNING
    }

    fn decode<'i, I>(values: &mut I) -> Result<Self, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let value = values.next().ok_or_else(headers::Error::invalid)?;
        value
            .to_str()
            .map_err(|_| headers::Error::invalid())?
            .parse()
            .map_err(|()| headers::Error::invalid())
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        if let Ok(value) = HeaderValue::from_str(&self.to_string()) {
            values.extend(iter::once(value));
        }
    }
}

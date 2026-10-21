//! The clause 4.6.3 lexical forms of `DateTime`, `Date` and `Time`, read character by character.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.6.3 pins each form completely: `DateTime` is
//! `YYYY-MM-DDThh:mm:ssZ`, `Date` is `YYYY-MM-DD` and `Time` is `hh:mm:ssZ`;
//! "all the referred components shall appear in the string; reduced
//! representations are not permitted"; the seconds may carry "a decimal point
//! and then one or more fractional digits, up to a maximum of six", for which
//! "in requests, also a comma instead of a decimal point may be used"; and the
//! trailing zone component "shall always be equal to the character 'Z'".
//!
//! This reader is the only place text becomes the components of a time value.
//! It accepts exactly those spellings and nothing a general RFC 3339 or ISO 8601
//! parser would add: no lowercase `t` or `z`, no space for the `T`, no numeric
//! offset, no missing seconds, no seventh fractional digit. What it hands on are
//! numbers; whether those numbers name a real calendar day or clock time is
//! decided by the value types that construct from them.

use std::borrow::Cow;
use thiserror::Error;

/// The most fractional-second digits clause 4.6.3 admits, which is also the
/// microsecond resolution every time value is held at.
pub const MAX_FRACTIONAL_DIGITS: usize = 6;

/// Why a text is not written in a clause 4.6.3 time value form.
///
/// The variants carry no text: refusals reach clients, and the value they name
/// reports its own type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum LexicalError {
    /// The text does not follow the form's component layout.
    #[error("is not written in the complete form its type defines")]
    Malformed,
    /// The seconds carry more fractional digits than clause 4.6.3 admits.
    #[error("carries more than {MAX_FRACTIONAL_DIGITS} fractional-second digits")]
    TooManyFractionalDigits,
    /// The trailing zone component is present and is not the character `Z`:
    /// a numeric offset, or the lowercase `z` RFC 3339 also admits.
    #[error("has a trailing zone designator other than the literal character 'Z'")]
    ZoneDesignator,
}

/// The calendar components of a `Date` or of the date half of a `DateTime`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DateComponents {
    /// The four-digit year, `0000` to `9999`.
    pub(crate) year: i32,
    /// The month as written, `00` to `99`.
    pub(crate) month: u32,
    /// The day of the month as written, `00` to `99`.
    pub(crate) day: u32,
}

/// The clock components of a `Time` or of the time half of a `DateTime`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimeComponents {
    /// The hour as written, `00` to `99`.
    pub(crate) hour: u32,
    /// The minute as written, `00` to `99`.
    pub(crate) minute: u32,
    /// The second as written, `00` to `99`.
    pub(crate) second: u32,
    /// The fraction of the second in microseconds, zero when none is written.
    pub(crate) microsecond: u32,
}

/// Read a `DateTime`: `YYYY-MM-DDThh:mm:ss[.ffffff]Z`.
///
/// # Errors
///
/// Returns the [`LexicalError`] naming the first way the text departs from the form.
pub(crate) fn date_time(text: &str) -> Result<(DateComponents, TimeComponents), LexicalError> {
    let mut cursor = Cursor::new(text);
    let date = cursor.date()?;
    cursor.expect(b'T')?;
    let time = cursor.time()?;
    cursor.zone()?;
    Ok((date, time))
}

/// Read a `Date`: `YYYY-MM-DD`.
///
/// # Errors
///
/// Returns [`LexicalError::Malformed`] when the text is not exactly that form.
pub(crate) fn date(text: &str) -> Result<DateComponents, LexicalError> {
    let mut cursor = Cursor::new(text);
    let date = cursor.date()?;
    cursor.end()?;
    Ok(date)
}

/// Read a `Time`: `hh:mm:ss[.ffffff]Z`.
///
/// # Errors
///
/// Returns the [`LexicalError`] naming the first way the text departs from the form.
pub(crate) fn time(text: &str) -> Result<TimeComponents, LexicalError> {
    let mut cursor = Cursor::new(text);
    let time = cursor.time()?;
    cursor.zone()?;
    Ok(time)
}

/// `text`, a spelling one of the readers above accepted, with a comma
/// separator written as the decimal point.
///
/// The comma is accepted "in requests" only (clause 4.6.3), so the spelling the
/// broker keeps and writes back always carries the point. In an accepted
/// spelling the separator is the only place a comma can stand.
pub(crate) fn with_decimal_point(text: &str) -> Cow<'_, str> {
    if text.contains(',') {
        Cow::Owned(text.replacen(',', ".", 1))
    } else {
        Cow::Borrowed(text)
    }
}

/// A forward-only reader over the bytes of a candidate time value.
///
/// Every byte the forms admit is ASCII, so a byte position is a character
/// position and a non-ASCII byte is simply one the form does not admit.
struct Cursor<'a> {
    /// The text being read.
    bytes: &'a [u8],
    /// The index of the next unread byte.
    position: usize,
}

impl<'a> Cursor<'a> {
    /// A reader positioned at the start of `text`.
    const fn new(text: &'a str) -> Cursor<'a> {
        Cursor {
            bytes: text.as_bytes(),
            position: 0,
        }
    }

    /// The unread bytes.
    fn rest(&self) -> &'a [u8] {
        self.bytes.get(self.position..).unwrap_or_default()
    }

    /// Consume `expected`, or refuse the text.
    fn expect(&mut self, expected: u8) -> Result<(), LexicalError> {
        match self.rest().first() {
            Some(&byte) if byte == expected => {
                self.position += 1;
                Ok(())
            }
            Some(_) | None => Err(LexicalError::Malformed),
        }
    }

    /// Consume exactly `count` decimal digits and return their value.
    fn digits(&mut self, count: usize) -> Result<u32, LexicalError> {
        let run = self.rest().get(..count).ok_or(LexicalError::Malformed)?;
        let value = run.iter().try_fold(0_u32, |value, &byte| {
            byte.is_ascii_digit()
                .then(|| value * 10 + u32::from(byte - b'0'))
                .ok_or(LexicalError::Malformed)
        })?;
        self.position += count;
        Ok(value)
    }

    /// Consume `YYYY-MM-DD`.
    fn date(&mut self) -> Result<DateComponents, LexicalError> {
        let year = self.digits(4)?;
        self.expect(b'-')?;
        let month = self.digits(2)?;
        self.expect(b'-')?;
        let day = self.digits(2)?;
        Ok(DateComponents {
            year: i32::try_from(year).map_err(|_| LexicalError::Malformed)?,
            month,
            day,
        })
    }

    /// Consume `hh:mm:ss` and the optional fraction.
    fn time(&mut self) -> Result<TimeComponents, LexicalError> {
        let hour = self.digits(2)?;
        self.expect(b':')?;
        let minute = self.digits(2)?;
        self.expect(b':')?;
        let second = self.digits(2)?;
        let microsecond = self.fraction()?;
        Ok(TimeComponents {
            hour,
            minute,
            second,
            microsecond,
        })
    }

    /// Consume the optional fraction of the second: a decimal point or, as the
    /// clause 4.6.3 compatibility form, a comma, then one to six digits.
    fn fraction(&mut self) -> Result<u32, LexicalError> {
        match self.rest().first() {
            Some(b'.' | b',') => self.position += 1,
            Some(_) | None => return Ok(0),
        }
        let width = self.rest().iter().take_while(|byte| byte.is_ascii_digit()).count();
        if width == 0 {
            return Err(LexicalError::Malformed);
        }
        if width > MAX_FRACTIONAL_DIGITS {
            return Err(LexicalError::TooManyFractionalDigits);
        }
        let written = self.digits(width)?;
        let scale = (width..MAX_FRACTIONAL_DIGITS).fold(1_u32, |scale, _| scale * 10);
        Ok(written * scale)
    }

    /// Consume the trailing `Z`, which has to end the text.
    ///
    /// A missing designator is a reduced representation and so malformed; a
    /// present one that is not `Z` is refused as the designator it is not,
    /// because that is what the client has to change.
    fn zone(&mut self) -> Result<(), LexicalError> {
        match self.rest() {
            b"Z" => {
                self.position += 1;
                Ok(())
            }
            [b'+' | b'-' | b'z', ..] => Err(LexicalError::ZoneDesignator),
            [..] => Err(LexicalError::Malformed),
        }
    }

    /// Refuse anything left unread.
    fn end(&self) -> Result<(), LexicalError> {
        if self.rest().is_empty() { Ok(()) } else { Err(LexicalError::Malformed) }
    }
}

#[cfg(test)]
mod tests {
    use crate::time::lexical::{DateComponents, LexicalError, TimeComponents, date, date_time, time, with_decimal_point};

    #[test]
    fn a_date_time_is_read_into_its_components() {
        let (read_date, read_time) = date_time("2026-03-04T05:06:07.123456Z").expect("a DateTime");
        pretty_assertions::assert_eq!(read_date, DateComponents { year: 2026, month: 3, day: 4 });
        pretty_assertions::assert_eq!(
            read_time,
            TimeComponents {
                hour: 5,
                minute: 6,
                second: 7,
                microsecond: 123_456
            }
        );
    }

    /// Clause 4.6.3: "one or more fractional digits, up to a maximum of six".
    #[test]
    fn a_short_fraction_is_scaled_to_microseconds() {
        pretty_assertions::assert_eq!(time("12:00:00.5Z").expect("one digit").microsecond, 500_000);
        pretty_assertions::assert_eq!(time("12:00:00.000001Z").expect("six digits").microsecond, 1);
        pretty_assertions::assert_eq!(time("12:00:00Z").expect("no fraction").microsecond, 0);
    }

    /// Clause 4.6.3: "In requests, also a comma instead of a decimal point may
    /// be used as separator for compatibility reasons."
    #[test]
    fn a_comma_separator_reads_as_the_decimal_point() {
        pretty_assertions::assert_eq!(date_time("2026-03-04T05:06:07,25Z"), date_time("2026-03-04T05:06:07.25Z"));
    }

    #[test]
    fn an_accepted_comma_spelling_is_kept_with_the_decimal_point() {
        pretty_assertions::assert_eq!(with_decimal_point("2026-03-04T05:06:07,25Z"), "2026-03-04T05:06:07.25Z");
        pretty_assertions::assert_eq!(with_decimal_point("2026-03-04T05:06:07.25Z"), "2026-03-04T05:06:07.25Z");
    }

    #[test]
    fn a_seventh_fractional_digit_is_refused_as_too_many() {
        for text in ["2026-03-04T05:06:07.1234567Z", "2026-03-04T05:06:07,123456789Z"] {
            pretty_assertions::assert_eq!(date_time(text), Err(LexicalError::TooManyFractionalDigits), "{text}");
        }
        pretty_assertions::assert_eq!(time("12:00:00.1234567Z"), Err(LexicalError::TooManyFractionalDigits));
    }

    /// Clause 4.6.3: the trailing component "shall always be equal to the
    /// character 'Z'", so an offset naming UTC is refused like any other.
    #[test]
    fn a_designator_other_than_z_is_refused_as_the_designator() {
        for text in [
            "2026-03-04T05:06:07+01:00",
            "2026-03-04T05:06:07+00:00",
            "2026-03-04T05:06:07-05:00",
            "2026-03-04T05:06:07z",
        ] {
            pretty_assertions::assert_eq!(date_time(text), Err(LexicalError::ZoneDesignator), "{text}");
        }
        pretty_assertions::assert_eq!(time("12:00:00+00:00"), Err(LexicalError::ZoneDesignator));
    }

    /// Clause 4.6.3: "All the referred components shall appear in the string;
    /// reduced representations are not permitted."
    #[test]
    fn a_reduced_or_otherwise_rearranged_spelling_is_malformed() {
        for text in [
            "2026-03-04T05:06Z",
            "2026-03-04T05:06:07",
            "2026-03-04",
            "2026-03-04t05:06:07Z",
            "2026-03-04 05:06:07Z",
            "2026-3-04T05:06:07Z",
            "26-03-04T05:06:07Z",
            "2026-03-04T05:06:07.Z",
            "2026-03-04T05:06:07,5,5Z",
            "2026-03-04T05:06:07ZZ",
            "+2026-03-04T05:06:07Z",
            "",
        ] {
            pretty_assertions::assert_eq!(date_time(text), Err(LexicalError::Malformed), "{text}");
        }
    }

    #[test]
    fn a_date_is_exactly_its_ten_characters() {
        pretty_assertions::assert_eq!(date("2026-04-12"), Ok(DateComponents { year: 2026, month: 4, day: 12 }));
        for text in ["2026-4-12", "2026-04-12Z", "2026-04-12T00:00:00Z", "20260412"] {
            pretty_assertions::assert_eq!(date(text), Err(LexicalError::Malformed), "{text}");
        }
    }

    #[test]
    fn a_time_without_its_designator_is_malformed() {
        pretty_assertions::assert_eq!(time("12:00:00"), Err(LexicalError::Malformed));
        pretty_assertions::assert_eq!(time("12:00Z"), Err(LexicalError::Malformed));
    }

    #[test]
    fn a_non_ascii_byte_is_malformed_rather_than_a_panic() {
        pretty_assertions::assert_eq!(date_time("2026-03-04T05:06:07.5é"), Err(LexicalError::Malformed));
        pretty_assertions::assert_eq!(time("１２:00:00Z"), Err(LexicalError::Malformed));
    }
}

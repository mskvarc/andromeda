//! Where a clause 4.6.3 time value written bare ends.
//!
//! Clause 4.9 writes its `dateTime`, `date` and `time` operands unquoted, and
//! each of the three opens with a digit run that a `Number` cannot continue
//! with: RFC 8259's `number` production carries neither `-` after its integer
//! digits nor `:` at all, and clause 4.6.2 makes every `AttrName` open with a
//! letter, so a digit run followed by either separator is a clause 4.6.3
//! literal rather than a number followed by punctuation.

use crate::filter::parser::operand::extent_while;

/// The byte length of the time value a digit run opens at the start of `text`, or `None` when the run opens none.
///
/// A run of ASCII digits opens a time value when a `-` or `:` and then an ASCII digit follow it. The value runs on from the end of the digits while:
/// - a `.` followed by a second `.` stops it, so the range separator stays outside;
/// - a `,` followed by ASCII digits and a `Z` is taken, as clause 4.6.3 admits a comma for the decimal point of the seconds in a request, and any other `,` stops it, so a value list member stays outside;
/// - an alphanumeric character or one of `-`, `:`, `.` and `+` is taken, wider than the conformant forms on purpose: a numeric zone offset belongs to the literal, so that the literal is refused as a malformed `dateTime` or `time` instead of being split;
/// - anything else stops it.
#[must_use]
pub(super) fn time_value_extent(text: &str) -> Option<usize> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    let rest = text.get(digits..)?;
    let mut following = rest.chars();
    let opens = digits > 0 && matches!(following.next(), Some('-' | ':')) && following.next().is_some_and(|character| character.is_ascii_digit());
    opens.then(|| digits + extent_while(rest, continues_time_value))
}

/// Whether `current`, followed by `following`, continues a time value.
fn continues_time_value(current: char, following: &str) -> bool {
    match current {
        '.' => !following.starts_with('.'),
        ',' => is_fraction_comma(following),
        other => other.is_alphanumeric() || matches!(other, '-' | ':' | '+'),
    }
}

/// Whether a `,` followed by `following` stands for the decimal point of the seconds of the time value it continues, rather than separating a value list.
///
/// Both a `DateTime` and a `Time` end in `Z`, so such a comma is followed by digits and the `Z`. No `Value` of a clause 4.9 `ValueList` is digits followed by `Z`, so a comma before them never opens a list member, after any operator.
fn is_fraction_comma(following: &str) -> bool {
    let after_digits = following.trim_start_matches(|character: char| character.is_ascii_digit());
    after_digits.len() < following.len() && after_digits.starts_with('Z')
}

#[cfg(test)]
mod tests {
    use crate::filter::parser::time_value_extent::time_value_extent;

    #[test]
    fn a_digit_run_opens_a_time_value_only_before_a_separator_and_a_digit() {
        pretty_assertions::assert_eq!(time_value_extent("2017-12-24..x"), Some(10));
        pretty_assertions::assert_eq!(time_value_extent("12:00:00,5Z,1"), Some(11));
        pretty_assertions::assert_eq!(time_value_extent("2017-12-24T12:00:00+01:00;"), Some(25));
        pretty_assertions::assert_eq!(time_value_extent("12"), None);
        pretty_assertions::assert_eq!(time_value_extent("12-x"), None);
    }

    #[test]
    fn no_time_value_opens_without_a_digit_run() {
        pretty_assertions::assert_eq!(time_value_extent("-12:00"), None);
        pretty_assertions::assert_eq!(time_value_extent(":00"), None);
        pretty_assertions::assert_eq!(time_value_extent(""), None);
    }

    #[test]
    fn a_comma_continues_a_time_value_only_before_digits_and_a_z() {
        pretty_assertions::assert_eq!(time_value_extent("2024-01-01T00:00:00,5Z"), Some(22));
        pretty_assertions::assert_eq!(time_value_extent("2024-01-15,2024-01-16"), Some(10));
        pretty_assertions::assert_eq!(time_value_extent("12:00:00Z,5"), Some(9));
    }
}

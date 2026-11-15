//! Writing a `NaiveDate` or a `NaiveTime` member through `#[serde(serialize_with = …)]`.
//!
//! A member typed as a date or a time of day is written as the NGSI-LD `Date`
//! or `Time` it names spells it (clause 4.6.3), so a serde model holding a bare
//! `chrono` value follows the same rules as every other path. `chrono`'s own
//! serde support is not enabled anywhere in the workspace, so a member that
//! names no adapter does not compile.

/// A calendar date.
pub mod date {
    use crate::time::date::NgsiLdDate;
    use chrono::NaiveDate;
    use serde::{Serialize, Serializer};

    /// Write the date as the clause 4.6.3 `Date` spells it, `YYYY-MM-DD`.
    ///
    /// # Errors
    ///
    /// Propagates whatever the serializer reports.
    pub fn serialize<S>(date: &NaiveDate, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        NgsiLdDate::from(*date).serialize(serializer)
    }
}

/// A time of day.
pub mod time_of_day {
    use crate::time::time_of_day::NgsiLdTime;
    use chrono::NaiveTime;
    use serde::{Serialize, Serializer};

    /// Write the time of day as the clause 4.6.3 `Time` spells it at the
    /// microsecond, `HH:MM:SS.ffffffZ`.
    ///
    /// # Errors
    ///
    /// Propagates whatever the serializer reports.
    pub fn serialize<S>(time: &NaiveTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        NgsiLdTime::normalise(*time).serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, NaiveTime};
    use serde::Serialize;

    #[derive(Serialize)]
    struct Calendar {
        #[serde(serialize_with = "crate::time::calendar_serde::date::serialize")]
        day: NaiveDate,
        #[serde(serialize_with = "crate::time::calendar_serde::time_of_day::serialize")]
        at: NaiveTime,
    }

    #[test]
    fn a_date_and_a_time_of_day_are_written_in_their_clause_4_6_3_spelling() {
        let calendar = Calendar {
            day: NaiveDate::from_ymd_opt(2024, 6, 5).expect("a calendar day"),
            at: NaiveTime::from_hms_micro_opt(12, 30, 0, 750_000).expect("a time of day"),
        };
        pretty_assertions::assert_eq!(
            serde_json::to_string(&calendar).expect("serializable"),
            r#"{"day":"2024-06-05","at":"12:30:00.750000Z"}"#
        );
    }

    #[test]
    fn a_time_of_day_is_written_at_the_microsecond() {
        let calendar = Calendar {
            day: NaiveDate::from_ymd_opt(2024, 6, 5).expect("a calendar day"),
            at: NaiveTime::from_hms_nano_opt(12, 30, 0, 123_456_789).expect("a time of day"),
        };
        pretty_assertions::assert_eq!(
            serde_json::to_string(&calendar).expect("serializable"),
            r#"{"day":"2024-06-05","at":"12:30:00.123456Z"}"#
        );
    }
}

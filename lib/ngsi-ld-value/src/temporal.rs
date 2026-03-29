use crate::{
    parsing::Parser,
    types::{TemporalValue, Value},
};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};

impl TemporalValue {
    /// Attempts to parse a string into a TemporalValue using heuristics.
    pub fn try_parse(s: &str) -> Option<Self> {
        let s = s.trim();

        // 1. Try ISO8601/RFC3339
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return Some(TemporalValue::DateTime(dt.with_timezone(&Utc)));
        }

        // 2. Common formats
        let formats = ["%Y-%m-%d %H:%M:%S", "%d/%m/%Y %H:%M:%S", "%Y-%m-%d", "%d/%m/%Y", "%Y"];

        for fmt in formats {
            if let Ok(dt) = DateTime::parse_from_str(s, fmt) {
                return Some(TemporalValue::DateTime(dt.with_timezone(&Utc)));
            }
            if let Ok(d) = NaiveDate::parse_from_str(s, fmt) {
                if let Some(dt) = d.and_hms_opt(0, 0, 0) {
                    return Some(TemporalValue::DateTime(Utc.from_utc_datetime(&dt)));
                }
            }
        }

        // 3. Heuristic for timestamps (seconds/millis/nanos)
        if let Some(n) = Parser::parse_integer(s) {
            return Self::from_timestamp_heuristic(n);
        }

        None
    }

    fn from_timestamp_heuristic(n: i64) -> Option<Self> {
        if n.abs() < 200_000_000 {
            return None; // Likely a year or small number
        }

        if n.abs() < 100_000_000_000 {
            // Seconds
            Utc.timestamp_opt(n, 0).single().map(TemporalValue::DateTime)
        } else if n.abs() < 100_000_000_000_000 {
            // Millis
            DateTime::from_timestamp_millis(n).map(TemporalValue::DateTime)
        } else {
            // Nanos
            let secs = n / 1_000_000_000;
            let nsecs = (n % 1_000_000_000) as u32;
            Utc.timestamp_opt(secs, nsecs).single().map(TemporalValue::DateTime)
        }
    }
}

impl Value {
    pub fn to_datetime(&self) -> DateTime<Utc> {
        match self {
            Value::Temporal(t) => match t {
                TemporalValue::DateTime(dt) | TemporalValue::Date(dt) | TemporalValue::Time(dt) => *dt,
            },
            Value::String(s) => TemporalValue::try_parse(s)
                .map(|t| match t {
                    TemporalValue::DateTime(dt) | TemporalValue::Date(dt) | TemporalValue::Time(dt) => dt,
                })
                .unwrap_or_else(Utc::now),
            _ => Utc::now(),
        }
    }

    pub fn to_date(&self) -> DateTime<Utc> {
        self.to_datetime()
    }

    pub fn to_time(&self) -> DateTime<Utc> {
        self.to_datetime()
    }
}

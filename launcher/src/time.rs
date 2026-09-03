use chrono::{SecondsFormat, Utc};

/// Returns a UTC timestamp suitable for all persisted protocol documents.
pub fn now_iso8601() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

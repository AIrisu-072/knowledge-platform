//! Field kinds of the audit catalog and the primitive checks behind them.
//! There is deliberately no free-text kind.

use serde::Deserialize;
use serde_json::Value;

/// Upper bound for any string in an envelope, in UTF-8 bytes.
pub const MAX_STRING_BYTES: usize = 512;
/// Upper bound for each principal part (issuer / principal id), in UTF-8 bytes.
pub const MAX_PRINCIPAL_PART_BYTES: usize = 256;
/// Upper bound for `uuid_list` values.
pub const MAX_UUID_LIST: usize = 100;
/// Upper bound for `identifier` values (control events), in UTF-8 bytes.
pub const MAX_IDENTIFIER_BYTES: usize = 256;
/// Upper bound for `identifier_list` values (control events).
pub const MAX_IDENTIFIER_LIST: usize = 32;
/// Upper bound for `code` values (`[a-z0-9_]{1,64}`).
pub const MAX_CODE_BYTES: usize = 64;
/// The nil UUID in canonical form.
pub const NIL_UUID: &str = "00000000-0000-0000-0000-000000000000";

/// Typed value shapes admitted in `details`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Uuid,
    NullableUuid,
    Counter,
    NullableCounter,
    PositiveCounter,
    Boolean,
    Enum,
    NullableEnum,
    EnumList,
    Digest,
    NullableDigest,
    Principal,
    LegacyTime,
    UtcTimestamp,
    UuidList,
    HexDigest,
    /// Control events only: a bounded name chosen by the Store (database role,
    /// event type, source URN, issuer). Non-empty, at most 256 bytes, no
    /// control characters.
    Identifier,
    /// Control events only: 1 to 32 distinct `identifier` values.
    IdentifierList,
    /// Control events only: a machine code `[a-z0-9_]{1,64}`.
    Code,
}

impl Kind {
    pub const ALL: [Self; 19] = [
        Self::Uuid,
        Self::NullableUuid,
        Self::Counter,
        Self::NullableCounter,
        Self::PositiveCounter,
        Self::Boolean,
        Self::Enum,
        Self::NullableEnum,
        Self::EnumList,
        Self::Digest,
        Self::NullableDigest,
        Self::Principal,
        Self::LegacyTime,
        Self::UtcTimestamp,
        Self::UuidList,
        Self::HexDigest,
        Self::Identifier,
        Self::IdentifierList,
        Self::Code,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Uuid => "uuid",
            Self::NullableUuid => "nullable_uuid",
            Self::Counter => "counter",
            Self::NullableCounter => "nullable_counter",
            Self::PositiveCounter => "positive_counter",
            Self::Boolean => "boolean",
            Self::Enum => "enum",
            Self::NullableEnum => "nullable_enum",
            Self::EnumList => "enum_list",
            Self::Digest => "digest",
            Self::NullableDigest => "nullable_digest",
            Self::Principal => "principal",
            Self::LegacyTime => "legacy_time",
            Self::UtcTimestamp => "utc_timestamp",
            Self::UuidList => "uuid_list",
            Self::HexDigest => "hex_digest",
            Self::Identifier => "identifier",
            Self::IdentifierList => "identifier_list",
            Self::Code => "code",
        }
    }

    /// Whether the kind requires a non-empty `values` list.
    pub const fn takes_values(self) -> bool {
        matches!(self, Self::Enum | Self::NullableEnum | Self::EnumList)
    }

    /// Kinds reserved for control events built by the Store or relay. They
    /// carry Store-chosen names, never source payload text, so relay-origin
    /// entries must not use them.
    pub const fn is_control_only(self) -> bool {
        matches!(self, Self::Identifier | Self::IdentifierList | Self::Code)
    }

    /// Checks `value` against this kind. `values` is the closed set for enum kinds.
    pub fn accepts(self, values: &[String], value: &Value) -> bool {
        let in_set = |v: &Value| v.as_str().is_some_and(|s| values.iter().any(|x| x == s));
        match self {
            Self::Uuid => value.as_str().is_some_and(is_uuid),
            Self::NullableUuid => value.is_null() || value.as_str().is_some_and(is_uuid),
            Self::Counter => is_counter(value),
            Self::NullableCounter => value.is_null() || is_counter(value),
            Self::PositiveCounter => value.as_i64().is_some_and(|n| n >= 1),
            Self::Boolean => value.is_boolean(),
            Self::Enum => in_set(value),
            Self::NullableEnum => value.is_null() || in_set(value),
            Self::EnumList => value.as_array().is_some_and(|items| {
                !items.is_empty()
                    && items.len() <= values.len()
                    && items.iter().all(in_set)
                    && items
                        .iter()
                        .enumerate()
                        .all(|(i, item)| !items[..i].contains(item))
            }),
            Self::Digest => is_digest(value),
            Self::NullableDigest => value.is_null() || is_digest(value),
            Self::Principal => is_legacy_principal(value),
            Self::LegacyTime => is_legacy_time(value),
            Self::UtcTimestamp => value.as_str().is_some_and(is_utc_timestamp),
            Self::UuidList => value.as_array().is_some_and(|items| {
                items.len() <= MAX_UUID_LIST
                    && items.iter().all(|item| item.as_str().is_some_and(is_uuid))
                    && items
                        .iter()
                        .enumerate()
                        .all(|(i, item)| !items[..i].contains(item))
            }),
            Self::HexDigest => value.as_str().is_some_and(is_hex_digest),
            Self::Identifier => value.as_str().is_some_and(is_identifier),
            Self::IdentifierList => value.as_array().is_some_and(|items| {
                !items.is_empty()
                    && items.len() <= MAX_IDENTIFIER_LIST
                    && items
                        .iter()
                        .all(|item| item.as_str().is_some_and(is_identifier))
                    && items
                        .iter()
                        .enumerate()
                        .all(|(i, item)| !items[..i].contains(item))
            }),
            Self::Code => value.as_str().is_some_and(is_code),
        }
    }
}

/// A control-event identifier: non-empty, at most 256 bytes, no control
/// characters.
pub fn is_identifier(text: &str) -> bool {
    !text.is_empty() && is_bounded_text(text, MAX_IDENTIFIER_BYTES)
}

/// A machine code: `[a-z0-9_]{1,64}`.
pub fn is_code(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_CODE_BYTES
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn is_counter(value: &Value) -> bool {
    value.as_i64().is_some_and(|n| n >= 0)
}

fn is_digest(value: &Value) -> bool {
    value.as_array().is_some_and(|bytes| {
        bytes.len() == 32
            && bytes
                .iter()
                .all(|byte| byte.as_u64().is_some_and(|b| b <= 255))
    })
}

/// Canonical lowercase hyphenated UUID text (nil allowed).
pub fn is_canonical_uuid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(b),
        })
}

/// Canonical lowercase hyphenated UUID text that is not the nil UUID.
pub fn is_uuid(text: &str) -> bool {
    is_canonical_uuid(text) && text != NIL_UUID
}

/// 64 lowercase hex characters (a SHA-256 digest).
pub fn is_hex_digest(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(is_lower_hex)
}

/// A W3C trace id: 32 lowercase hex characters, not all zero.
pub fn is_w3c_trace_id(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(is_lower_hex) && text.bytes().any(|b| b != b'0')
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

/// Bounded text: at most `max` UTF-8 bytes and no control characters.
pub fn is_bounded_text(text: &str, max: usize) -> bool {
    text.len() <= max && !text.chars().any(char::is_control)
}

/// A principal part: non-empty bounded text without control characters.
pub fn is_principal_part(text: &str) -> bool {
    !text.is_empty() && is_bounded_text(text, MAX_PRINCIPAL_PART_BYTES)
}

/// Legacy `{identityProvider, principalId}` object with exactly these keys.
fn is_legacy_principal(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == 2
            && ["identityProvider", "principalId"].iter().all(|key| {
                object
                    .get(*key)
                    .and_then(Value::as_str)
                    .is_some_and(is_principal_part)
            })
    })
}

/// The legacy `time` serde tuple: `[year, ordinal, hour, minute, second,
/// nanosecond, offset_h, offset_m, offset_s]`, all JSON integers, forming a
/// valid date/time with a single-signed offset.
pub fn is_legacy_time(value: &Value) -> bool {
    let Some(items) = value.as_array() else {
        return false;
    };
    if items.len() != 9 {
        return false;
    }
    let mut parts = [0_i64; 9];
    for (slot, item) in parts.iter_mut().zip(items) {
        match item.as_i64() {
            Some(n) => *slot = n,
            None => return false,
        }
    }
    let [
        year,
        ordinal,
        hour,
        minute,
        second,
        nanos,
        off_h,
        off_m,
        off_s,
    ] = parts;
    let (Ok(year), Ok(ordinal)) = (i32::try_from(year), u16::try_from(ordinal)) else {
        return false;
    };
    let date_ok =
        (-9999..=9999).contains(&year) && time::Date::from_ordinal_date(year, ordinal).is_ok();
    let time_ok = (0..=23).contains(&hour)
        && (0..=59).contains(&minute)
        && (0..=59).contains(&second)
        && (0..=999_999_999).contains(&nanos);
    let offset_ok = (-25..=25).contains(&off_h)
        && (-59..=59).contains(&off_m)
        && (-59..=59).contains(&off_s)
        && !([off_h, off_m, off_s].iter().any(|n| *n < 0)
            && [off_h, off_m, off_s].iter().any(|n| *n > 0));
    date_ok && time_ok && offset_ok
}

/// `YYYY-MM-DDTHH:MM:SS.ffffffZ` naming a valid UTC instant.
pub fn is_utc_timestamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 27 {
        return false;
    }
    let layout_ok = bytes.iter().enumerate().all(|(i, b)| match i {
        4 | 7 => *b == b'-',
        10 => *b == b'T',
        13 | 16 => *b == b':',
        19 => *b == b'.',
        26 => *b == b'Z',
        _ => b.is_ascii_digit(),
    });
    if !layout_ok {
        return false;
    }
    let number = |range: std::ops::Range<usize>| -> u32 {
        bytes[range]
            .iter()
            .fold(0, |acc, b| acc * 10 + u32::from(b - b'0'))
    };
    let (year, month, day) = (number(0..4), number(5..7), number(8..10));
    let (hour, minute, second) = (number(11..13), number(14..16), number(17..19));
    let micros = number(20..26);
    let Some(month) = u8::try_from(month)
        .ok()
        .and_then(|m| time::Month::try_from(m).ok())
    else {
        return false;
    };
    let (Ok(year), Ok(day)) = (i32::try_from(year), u8::try_from(day)) else {
        return false;
    };
    year >= 1
        && time::Date::from_calendar_date(year, month, day).is_ok()
        && hour <= 23
        && minute <= 59
        && second <= 59
        && micros <= 999_999
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn uuid_forms() {
        assert!(is_uuid("0199a1b2-0000-7000-8000-00000000000a"));
        assert!(!is_uuid(NIL_UUID));
        assert!(is_canonical_uuid(NIL_UUID));
        assert!(!is_uuid("0199A1B2-0000-7000-8000-00000000000A"));
        assert!(!is_uuid("0199a1b200007000800000000000000a"));
        assert!(!is_uuid("{0199a1b2-0000-7000-8000-00000000000a}"));
    }

    #[test]
    fn counters_reject_floats_negative_and_out_of_range() {
        assert!(Kind::Counter.accepts(&[], &json!(0)));
        assert!(Kind::Counter.accepts(&[], &json!(i64::MAX)));
        assert!(!Kind::Counter.accepts(&[], &json!(-1)));
        assert!(!Kind::Counter.accepts(&[], &json!(u64::MAX)));
        assert!(!Kind::Counter.accepts(&[], &json!(1.0)));
        assert!(!Kind::PositiveCounter.accepts(&[], &json!(0)));
        assert!(Kind::NullableCounter.accepts(&[], &Value::Null));
    }

    #[test]
    fn legacy_time_rules() {
        assert!(is_legacy_time(&json!([2026, 280, 1, 2, 3, 4, 0, 0, 0])));
        assert!(is_legacy_time(&json!([
            2024,
            366,
            23,
            59,
            59,
            999_999_999,
            9,
            0,
            0
        ])));
        assert!(is_legacy_time(&json!([2026, 1, 0, 0, 0, 0, -5, -30, 0])));
        assert!(!is_legacy_time(&json!([2026, 366, 0, 0, 0, 0, 0, 0, 0])));
        assert!(!is_legacy_time(&json!([2026, 1, 24, 0, 0, 0, 0, 0, 0])));
        assert!(!is_legacy_time(&json!([2026, 1, 0, 0, 60, 0, 0, 0, 0])));
        assert!(!is_legacy_time(&json!([2026, 1, 0, 0, 0, 0, 0, -30, 15])));
        assert!(!is_legacy_time(&json!([2026, 1, 0, 0, 0, 0, 26, 0, 0])));
        assert!(!is_legacy_time(&json!([2026, 1, 0, 0, 0, 0.0, 0, 0, 0])));
        assert!(!is_legacy_time(&json!([2026, 1, 0, 0, 0, 0, 0, 0])));
    }

    #[test]
    fn utc_timestamp_rules() {
        assert!(is_utc_timestamp("2026-10-07T01:02:03.123456Z"));
        assert!(is_utc_timestamp("2024-02-29T23:59:59.999999Z"));
        assert!(!is_utc_timestamp("2026-02-29T00:00:00.000000Z"));
        assert!(!is_utc_timestamp("2026-10-07T01:02:03.123Z"));
        assert!(!is_utc_timestamp("2026-10-07T01:02:03.123456+00:00"));
        assert!(!is_utc_timestamp("2026-10-07T24:00:00.000000Z"));
    }

    #[test]
    fn enum_list_is_unique_and_closed() {
        let values = vec!["a".to_owned(), "b".to_owned()];
        assert!(Kind::EnumList.accepts(&values, &json!(["a", "b"])));
        assert!(!Kind::EnumList.accepts(&values, &json!([])));
        assert!(!Kind::EnumList.accepts(&values, &json!(["a", "a"])));
        assert!(!Kind::EnumList.accepts(&values, &json!(["c"])));
    }

    #[test]
    fn digest_and_text_rules() {
        assert!(Kind::Digest.accepts(&[], &Value::Array(vec![json!(255); 32])));
        assert!(!Kind::Digest.accepts(&[], &Value::Array(vec![json!(256); 32])));
        assert!(!Kind::Digest.accepts(&[], &Value::Array(vec![json!(0); 31])));
        assert!(is_hex_digest(&"a".repeat(64)));
        assert!(!is_hex_digest(&"A".repeat(64)));
        assert!(is_w3c_trace_id("4bf92f3577b34da6a3ce929d0e0e4736"));
        assert!(!is_w3c_trace_id(&"0".repeat(32)));
        assert!(!is_principal_part(""));
        assert!(!is_principal_part("a\u{7}"));
        assert!(!is_principal_part(&"é".repeat(129)));
        assert!(is_principal_part(&"é".repeat(128)));
    }

    #[test]
    fn control_kinds_are_bounded() {
        assert!(Kind::Identifier.accepts(&[], &json!("audit_store_reader")));
        assert!(Kind::Identifier.accepts(&[], &json!("é".repeat(128))));
        assert!(!Kind::Identifier.accepts(&[], &json!("é".repeat(129))));
        assert!(!Kind::Identifier.accepts(&[], &json!("")));
        assert!(!Kind::Identifier.accepts(&[], &json!("a\nb")));
        assert!(!Kind::Identifier.accepts(&[], &json!(1)));
        assert!(Kind::IdentifierList.accepts(&[], &json!(["a", "b"])));
        assert!(!Kind::IdentifierList.accepts(&[], &json!([])));
        assert!(!Kind::IdentifierList.accepts(&[], &json!(["a", "a"])));
        let many: Vec<String> = (0..=MAX_IDENTIFIER_LIST).map(|i| i.to_string()).collect();
        assert!(!Kind::IdentifierList.accepts(&[], &json!(many)));
        assert!(Kind::Code.accepts(&[], &json!("delivery_unknown_at_limit")));
        assert!(!Kind::Code.accepts(&[], &json!("Upper")));
        assert!(!Kind::Code.accepts(&[], &json!("has space")));
        assert!(!Kind::Code.accepts(&[], &json!("a".repeat(65))));
        assert!(Kind::ALL.iter().filter(|k| k.is_control_only()).count() == 3);
    }
}

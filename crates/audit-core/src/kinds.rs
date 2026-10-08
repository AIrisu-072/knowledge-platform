//! Field kinds of the audit catalog and the primitive checks behind them.
//! There is deliberately no free-text kind: every string kind is a closed
//! grammar or a closed set, including the control-only kinds that record
//! reader-supplied filters and Store-chosen names.

use serde::Deserialize;
use serde_json::Value;

/// Upper bound for any string in an envelope, in UTF-8 bytes.
pub const MAX_STRING_BYTES: usize = 512;
/// Upper bound for each principal part (issuer / principal id), in UTF-8 bytes.
pub const MAX_PRINCIPAL_PART_BYTES: usize = 256;
/// Upper bound for `uuid_list` values.
pub const MAX_UUID_LIST: usize = 100;
/// Upper bound for `event_type_list` and `source_list` values (control
/// events), as for intent filter event types (design §10.3). Retention
/// selectors are bounded by it too.
pub const MAX_CONTROL_LIST: usize = 16;
/// Upper bound for `db_role` values: PostgreSQL's NAMEDATALEN - 1.
pub const MAX_DB_ROLE_BYTES: usize = 63;
/// Upper bound for event type names, in bytes.
pub const MAX_EVENT_TYPE_BYTES: usize = 128;
/// Upper bound for `code` values (`[a-z0-9_]{1,64}`).
pub const MAX_CODE_BYTES: usize = 64;
/// The nil UUID in canonical form.
pub const NIL_UUID: &str = "00000000-0000-0000-0000-000000000000";
/// Upper bound of the `safe_counter` kinds: 2^53 - 1, the largest integer a
/// JSON number keeps exactly in every IEEE 754 reader (JavaScript
/// `Number.MAX_SAFE_INTEGER`). Producers that bound a counter by it (the
/// Document read-state revision, migration 0012 CHECK) use these kinds.
pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

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
    NullableHexDigest,
    NullableUtcTimestamp,
    /// Null or at least 1.
    NullablePositiveCounter,
    /// Control events only: a resource id as it appears in envelopes, a
    /// canonical lowercase UUID (nil included: `authorization.denied` rows)
    /// or exactly `audit-store`.
    ResourceRef,
    /// Control events only: an event type name in the catalog grammar
    /// `[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+`, at most 128 bytes (grammar only,
    /// for version skew).
    EventType,
    /// Control events only: 1 to 16 distinct `event_type` values.
    EventTypeList,
    /// Control events only: one of the catalog's sources (every adapter
    /// source plus the two control sources). The loader derives the closed
    /// set; the catalog entry lists no values.
    SourceUrn,
    /// Control events only: 1 to 16 distinct `source_urn` values.
    SourceList,
    /// Control events only: a PostgreSQL role name that needs no quoting,
    /// `[a-z_][a-z0-9_$]{0,62}` (the Store refuses other login names).
    DbRole,
    /// Control events only: `db_role` or null.
    NullableDbRole,
    /// Control events only: one part (issuer or principal id) of a principal,
    /// under the principal charset rule ([`is_principal_part`]).
    PrincipalRef,
    /// Control events only: the canonical decimal text of a PostgreSQL int8
    /// (`0` or `-?[1-9][0-9]*` within the i64 range), for fingerprint parts.
    Int8Text,
    /// Control events only: a machine code `[a-z0-9_]{1,64}`.
    Code,
    /// 0 to [`MAX_SAFE_INTEGER`] (a counter the producer bounds to the JSON
    /// safe integer range).
    SafeCounter,
    /// 1 to [`MAX_SAFE_INTEGER`].
    PositiveSafeCounter,
}

impl Kind {
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
            Self::NullableHexDigest => "nullable_hex_digest",
            Self::NullableUtcTimestamp => "nullable_utc_timestamp",
            Self::NullablePositiveCounter => "nullable_positive_counter",
            Self::ResourceRef => "resource_ref",
            Self::EventType => "event_type",
            Self::EventTypeList => "event_type_list",
            Self::SourceUrn => "source_urn",
            Self::SourceList => "source_list",
            Self::DbRole => "db_role",
            Self::NullableDbRole => "nullable_db_role",
            Self::PrincipalRef => "principal_ref",
            Self::Int8Text => "int8_text",
            Self::Code => "code",
            Self::SafeCounter => "safe_counter",
            Self::PositiveSafeCounter => "positive_safe_counter",
        }
    }

    /// Whether the kind requires a non-empty `values` list in the catalog.
    pub const fn takes_values(self) -> bool {
        matches!(self, Self::Enum | Self::NullableEnum | Self::EnumList)
    }

    /// Whether the loader derives the kind's `values` (the catalog's
    /// sources) instead of reading them from the entry.
    pub const fn derives_values(self) -> bool {
        matches!(self, Self::SourceUrn | Self::SourceList)
    }

    /// Kinds reserved for control events built by the Store or relay. Relay
    /// entries keep their own vocabulary, so they must not use them.
    pub const fn is_control_only(self) -> bool {
        matches!(
            self,
            Self::ResourceRef
                | Self::EventType
                | Self::EventTypeList
                | Self::SourceUrn
                | Self::SourceList
                | Self::DbRole
                | Self::NullableDbRole
                | Self::PrincipalRef
                | Self::Int8Text
                | Self::Code
        )
    }

    /// Checks `value` against this kind. `values` is the closed set for enum
    /// and source kinds.
    pub fn accepts(self, values: &[String], value: &Value) -> bool {
        let in_set = |v: &Value| v.as_str().is_some_and(|s| values.iter().any(|x| x == s));
        let text = |check: fn(&str) -> bool| value.as_str().is_some_and(check);
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
            Self::NullableHexDigest => value.is_null() || value.as_str().is_some_and(is_hex_digest),
            Self::NullableUtcTimestamp => {
                value.is_null() || value.as_str().is_some_and(is_utc_timestamp)
            }
            Self::NullablePositiveCounter => {
                value.is_null() || value.as_i64().is_some_and(|n| n >= 1)
            }
            Self::ResourceRef => text(is_resource_ref),
            Self::EventType => text(is_event_type),
            Self::EventTypeList => {
                is_control_list(value, |item| item.as_str().is_some_and(is_event_type))
            }
            Self::SourceUrn => in_set(value),
            Self::SourceList => is_control_list(value, in_set),
            Self::DbRole => text(is_db_role),
            Self::NullableDbRole => value.is_null() || text(is_db_role),
            Self::PrincipalRef => text(is_principal_part),
            Self::Int8Text => text(is_int8_text),
            Self::Code => text(is_code),
            Self::SafeCounter => is_safe_counter(value, 0),
            Self::PositiveSafeCounter => is_safe_counter(value, 1),
        }
    }
}

/// A JSON integer (never `1.0` or `1e0`) in `min..=MAX_SAFE_INTEGER`.
fn is_safe_counter(value: &Value, min: i64) -> bool {
    value
        .as_i64()
        .is_some_and(|n| (min..=MAX_SAFE_INTEGER).contains(&n))
}

/// 1 to [`MAX_CONTROL_LIST`] distinct items that all pass `item`.
fn is_control_list(value: &Value, item: impl Fn(&Value) -> bool) -> bool {
    value.as_array().is_some_and(|items| {
        !items.is_empty()
            && items.len() <= MAX_CONTROL_LIST
            && items.iter().all(item)
            && items
                .iter()
                .enumerate()
                .all(|(i, member)| !items[..i].contains(member))
    })
}

/// A resource id as recorded in envelopes: a canonical lowercase UUID (nil
/// included) or exactly `audit-store`.
pub fn is_resource_ref(text: &str) -> bool {
    is_canonical_uuid(text) || text == crate::catalog::AUDIT_STORE_RESOURCE_ID
}

/// A PostgreSQL role name that needs no quoting:
/// `[a-z_][a-z0-9_$]{0,62}`.
pub fn is_db_role(text: &str) -> bool {
    let mut bytes = text.bytes();
    text.len() <= MAX_DB_ROLE_BYTES
        && bytes
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b == b'_')
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'$')
}

/// The canonical decimal rendering of an int8 (what PostgreSQL's
/// `int8::text` produces).
pub fn is_int8_text(text: &str) -> bool {
    text.parse::<i64>().is_ok_and(|n| n.to_string() == text)
}

/// An event type name in the catalog grammar
/// `[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+`, at most 128 bytes.
pub fn is_event_type(text: &str) -> bool {
    text.len() <= MAX_EVENT_TYPE_BYTES
        && text.split('.').count() >= 2
        && text.split('.').all(|part| {
            let mut bytes = part.bytes();
            bytes.next().is_some_and(|b| b.is_ascii_lowercase())
                && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
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

/// A principal part (issuer or principal id): at most 256 UTF-8 bytes, not
/// empty, no leading or trailing Unicode `White_Space` (so not whitespace
/// only), and none of:
///
/// - `General_Category=Cc` (U+0000–001F, U+007F–009F);
/// - `Bidi_Control` (U+061C, U+200E, U+200F, U+202A–202E, U+2066–2069);
/// - U+2028 LINE SEPARATOR and U+2029 PARAGRAPH SEPARATOR;
/// - U+FEFF (BOM / zero width no-break space);
/// - the TAG block U+E0000–E007F;
/// - noncharacters (U+FDD0–FDEF and U+xxFFFE / U+xxFFFF on every plane).
///
/// Other format characters (e.g. ZWJ) stay allowed: identifiers may need
/// them and a rejected staging row is quarantined forever.
pub fn is_principal_part(text: &str) -> bool {
    text.len() <= MAX_PRINCIPAL_PART_BYTES
        && !text.starts_with(char::is_whitespace)
        && !text.ends_with(char::is_whitespace)
        && !text.is_empty()
        && !text.chars().any(is_forbidden_in_principal)
}

fn is_forbidden_in_principal(c: char) -> bool {
    let code = u32::from(c);
    c.is_control()
        || matches!(
            code,
            0x061C
                | 0x200E
                | 0x200F
                | 0x202A..=0x202E
                | 0x2066..=0x2069
                | 0x2028
                | 0x2029
                | 0xFEFF
                | 0xE0000..=0xE007F
                | 0xFDD0..=0xFDEF
        )
        || code & 0xFFFE == 0xFFFE
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
    fn safe_counters_stop_at_the_json_safe_integer() {
        assert_eq!(MAX_SAFE_INTEGER, (1_i64 << 53) - 1);
        let accepts = |kind: Kind, value: Value| kind.accepts(&[], &value);
        assert!(accepts(Kind::SafeCounter, json!(0)));
        assert!(accepts(Kind::SafeCounter, json!(MAX_SAFE_INTEGER)));
        assert!(accepts(Kind::PositiveSafeCounter, json!(1)));
        assert!(accepts(Kind::PositiveSafeCounter, json!(MAX_SAFE_INTEGER)));
        for kind in [Kind::SafeCounter, Kind::PositiveSafeCounter] {
            for bad in [
                json!(-1),
                json!(MAX_SAFE_INTEGER + 1),
                json!(i64::MAX),
                json!(u64::MAX),
                json!(1.0),
                json!("1"),
                Value::Null,
                json!(true),
            ] {
                assert!(!accepts(kind, bad.clone()), "{kind:?} {bad}");
            }
            assert!(!kind.is_control_only() && !kind.takes_values());
        }
        assert!(!accepts(Kind::PositiveSafeCounter, json!(0)));
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
    fn control_kinds_are_closed() {
        let none: &[String] = &[];
        let accepts = |kind: Kind, value: Value| kind.accepts(none, &value);
        // resource_ref: a canonical UUID (nil included) or exactly "audit-store".
        for ok in [
            json!("audit-store"),
            json!(NIL_UUID),
            json!("0199a1b2-0000-7000-8000-00000000000a"),
        ] {
            assert!(accepts(Kind::ResourceRef, ok.clone()), "{ok}");
        }
        for bad in [
            json!("Audit-Store"),
            json!("audit-store/x"),
            json!("0199A1B2-0000-7000-8000-00000000000A"),
            json!("Customer ACME merger codename FALCON, card 4111 1111 1111 1111"),
            json!(""),
            json!(1),
        ] {
            assert!(!accepts(Kind::ResourceRef, bad.clone()), "{bad}");
        }
        // event_type / event_type_list: the catalog grammar, at most 128 bytes.
        let longest = format!("{}.{}", "a".repeat(63), "b".repeat(64));
        assert!(accepts(Kind::EventType, json!("document.created")));
        assert!(accepts(Kind::EventType, json!(longest)));
        for bad in [
            "document",
            "Document.created",
            "document.created ",
            "a\u{2028}.b",
            &format!("{longest}c"),
        ] {
            assert!(!accepts(Kind::EventType, json!(bad)), "{bad:?}");
        }
        assert!(accepts(
            Kind::EventTypeList,
            json!(["document.created", "folder.moved"])
        ));
        let full: Vec<String> = (0..MAX_CONTROL_LIST).map(|i| format!("t.e{i}")).collect();
        assert!(accepts(Kind::EventTypeList, json!(full)));
        let over: Vec<String> = (0..=MAX_CONTROL_LIST).map(|i| format!("t.e{i}")).collect();
        for bad in [
            json!([]),
            json!(["document.created", "document.created"]),
            json!(over),
            json!(["a sentence of free text that a reader typed into the filter"]),
            json!("document.created"),
        ] {
            assert!(!accepts(Kind::EventTypeList, bad.clone()), "{bad}");
        }
        // db_role: a PostgreSQL identifier that needs no quoting.
        for ok in ["audit_store_reader", "_x", "a$b", &"r".repeat(63)] {
            assert!(accepts(Kind::DbRole, json!(ok)), "{ok:?}");
        }
        for bad in [
            "",
            "Audit",
            "1abc",
            "a b",
            "a-b",
            "op\u{202e}",
            "$a",
            &"r".repeat(64),
        ] {
            assert!(!accepts(Kind::DbRole, json!(bad)), "{bad:?}");
        }
        assert!(accepts(Kind::NullableDbRole, Value::Null));
        assert!(!accepts(Kind::NullableDbRole, json!("Odd Role")));
        // principal_ref: the principal charset rule.
        for ok in ["poc-human", "名前", "synthetic-idp"] {
            assert!(accepts(Kind::PrincipalRef, json!(ok)), "{ok:?}");
        }
        for bad in [
            "admin\u{202e}nimda",
            "\u{feff}",
            " x",
            "poc\u{e0041}",
            "a\u{2028}b",
            "",
        ] {
            assert!(!accepts(Kind::PrincipalRef, json!(bad)), "{bad:?}");
        }
        // int8_text: the canonical decimal rendering of an int8.
        for ok in [
            "0",
            "1",
            "-1",
            "4294967295",
            "9223372036854775807",
            "-9223372036854775808",
        ] {
            assert!(accepts(Kind::Int8Text, json!(ok)), "{ok:?}");
        }
        for bad in [
            "",
            "01",
            "-0",
            "+1",
            "1.0",
            "1e3",
            " 1",
            "9223372036854775808",
            "0x10",
        ] {
            assert!(!accepts(Kind::Int8Text, json!(bad)), "{bad:?}");
        }
        assert!(!accepts(Kind::Int8Text, json!(1)));
        // Source kinds use the closed set the loader derives from the catalog.
        let sources = vec![
            "urn:knowledge-platform:audit-store".to_owned(),
            "urn:knowledge-platform:document-platform".to_owned(),
        ];
        assert!(Kind::SourceUrn.accepts(&sources, &json!(sources[1])));
        assert!(
            !Kind::SourceUrn.accepts(&sources, &json!("urn:knowledge-platform:search-platform"))
        );
        assert!(!Kind::SourceUrn.accepts(&[], &json!(sources[1])));
        assert!(Kind::SourceList.accepts(&sources, &json!(sources)));
        for bad in [
            json!([]),
            json!([sources[0], sources[0]]),
            json!(["urn:other"]),
        ] {
            assert!(!Kind::SourceList.accepts(&sources, &bad), "{bad}");
        }
        // nullable_positive_counter: null or at least 1.
        assert!(accepts(Kind::NullablePositiveCounter, Value::Null));
        assert!(accepts(Kind::NullablePositiveCounter, json!(1)));
        assert!(!accepts(Kind::NullablePositiveCounter, json!(0)));
        // code.
        assert!(accepts(Kind::Code, json!("delivery_unknown_at_limit")));
        assert!(!accepts(Kind::Code, json!("Upper")));
        assert!(!accepts(Kind::Code, json!("has space")));
        assert!(!accepts(Kind::Code, json!("a".repeat(65))));
        for kind in [
            Kind::ResourceRef,
            Kind::EventType,
            Kind::EventTypeList,
            Kind::SourceUrn,
            Kind::SourceList,
            Kind::DbRole,
            Kind::NullableDbRole,
            Kind::PrincipalRef,
            Kind::Int8Text,
            Kind::Code,
        ] {
            assert!(kind.is_control_only(), "{kind:?}");
        }
        assert!(!Kind::NullablePositiveCounter.is_control_only());
        assert!(!Kind::NullableHexDigest.is_control_only());
        assert!(Kind::NullableHexDigest.accepts(&[], &Value::Null));
        assert!(!Kind::NullableHexDigest.accepts(&[], &json!("ab")));
        assert!(Kind::NullableUtcTimestamp.accepts(&[], &json!("2026-10-07T01:02:03.000000Z")));
        assert!(!Kind::NullableUtcTimestamp.accepts(&[], &json!("2026-10-07")));
    }

    #[test]
    fn principal_parts_refuse_invisible_and_spoofing_characters() {
        for ok in [
            "poc-human",
            "service",
            "a b",
            "名前",
            "e\u{301}",
            "a\u{200d}b",
            "\"quoted\"",
        ] {
            assert!(is_principal_part(ok), "{ok:?}");
        }
        for bad in [
            "",
            " ",
            "\u{3000}",
            " poc-human",
            "poc-human ",
            "poc-human\u{a0}",
            "\u{85}x",
            "poc\u{7}human",
            "poc\u{9f}",
            "\u{61c}x",
            "a\u{200e}",
            "a\u{200f}",
            "\u{202a}a",
            "admin\u{202e}nimda",
            "\u{2066}a",
            "\u{2069}a",
            "a\u{2028}b",
            "a\u{2029}b",
            "\u{feff}poc",
            "poc\u{e0041}",
            "\u{e007f}x",
            "\u{fdd0}x",
            "x\u{fdef}",
            "x\u{fffe}",
            "x\u{ffff}",
            "x\u{1fffe}",
            "x\u{10ffff}",
        ] {
            assert!(!is_principal_part(bad), "{bad:?}");
        }
        assert!(is_principal_part(&"p".repeat(MAX_PRINCIPAL_PART_BYTES)));
        assert!(!is_principal_part(
            &"p".repeat(MAX_PRINCIPAL_PART_BYTES + 1)
        ));
    }
}

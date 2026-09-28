use document_application::{
    ApplicationError, CursorBinding, CursorPosition, DocumentSort, QueryKind, decode_cursor,
    encode_cursor, validate_page_size,
};
use uuid::Uuid;

fn binding() -> CursorBinding {
    CursorBinding {
        kind: QueryKind::Published,
        sort: DocumentSort::CreatedAtDesc,
        filter_fingerprint: "filter".into(),
        principal_fingerprint: "issuer-bound-principal".into(),
        access_revision: 7,
    }
}

#[test]
fn page_size_rejects_zero_and_values_above_200() {
    assert_eq!(validate_page_size(1).unwrap(), 1);
    assert_eq!(validate_page_size(200).unwrap(), 200);
    assert!(matches!(
        validate_page_size(0),
        Err(ApplicationError::Validation(_))
    ));
    assert!(matches!(
        validate_page_size(201),
        Err(ApplicationError::Validation(_))
    ));
}

#[test]
fn cursor_binds_scope_sort_filter_principal_and_access_revision() {
    let binding = binding();
    let position = CursorPosition {
        document_id: Uuid::now_v7(),
        sort_time_micros: Some(1_000_000),
        sort_title: None,
    };
    let token = encode_cursor(&binding, &position).unwrap();
    assert_eq!(decode_cursor(&token, &binding).unwrap(), position);
    for stale in [
        CursorBinding {
            kind: QueryKind::History,
            ..binding.clone()
        },
        CursorBinding {
            sort: DocumentSort::TitleAsc,
            ..binding.clone()
        },
        CursorBinding {
            filter_fingerprint: "another-filter".into(),
            ..binding.clone()
        },
        CursorBinding {
            principal_fingerprint: "another-principal".into(),
            ..binding.clone()
        },
        CursorBinding {
            access_revision: 8,
            ..binding.clone()
        },
    ] {
        assert_eq!(
            decode_cursor(&token, &stale),
            Err(ApplicationError::CursorStale)
        );
    }
}

#[test]
fn malformed_and_oversize_cursor_is_validation_failure() {
    let binding = binding();
    for token in ["not-hex", "0", "7b7d", &"a".repeat(8193)] {
        assert!(matches!(
            decode_cursor(token, &binding),
            Err(ApplicationError::Validation(_))
        ));
    }
}

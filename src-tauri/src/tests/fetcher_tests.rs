use crate::email_cache::{uid_vec_to_set, FetchResult};
use std::collections::HashSet;

#[test]
fn fetch_result_count_returns_expected_value() {
    assert_eq!(FetchResult::EmptyMailbox.count(), 0);
    assert_eq!(FetchResult::Populated.count(), 0);
    assert_eq!(
        FetchResult::Fetched {
            count: 7,
            total_emails: 42,
        }
        .count(),
        7
    );
    assert_eq!(
        FetchResult::Fetched {
            count: 7,
            total_emails: 42,
        }
        .total_emails(),
        42
    );
}

#[test]
fn uid_vec_to_set_contains_all_values() {
    let mut uids = HashSet::new();
    uids.insert(42);
    uids.insert(7);
    uids.insert(19);

    let rendered = uid_vec_to_set(&uids);
    let mut parts: Vec<u32> = rendered
        .split(',')
        .map(|x| x.parse::<u32>().expect("uid should be numeric"))
        .collect();

    parts.sort_unstable();

    assert_eq!(parts, vec![7, 19, 42]);
}

#[test]
fn uid_vec_to_set_empty_hash_set_renders_empty_string() {
    let uids = HashSet::new();
    assert_eq!(uid_vec_to_set(&uids), "");
}

//! Smoke tests for the assay prelude — `expect()` matchers + JSON helpers.

use anvil_test::assay::*;
use anvil_test::dataset;
use serde_json::json;

#[test]
fn expect_to_be_compares_with_equality() {
    expect(2 + 2).to_be(4);
    expect("hello").to_be("hello");
    expect(vec![1, 2, 3]).to_be(vec![1, 2, 3]);
}

#[test]
#[should_panic(expected = "expected 5 NOT to equal 5")]
fn expect_not_to_be_panics_on_match() {
    expect(5).not().to_be(5);
}

#[test]
fn expect_string_matchers() {
    expect("hello world").to_contain("world");
    expect("hello world").to_start_with("hello");
    expect("hello world").to_end_with("world");
    expect("hello world").to_match(r"^hello\s\w+$");
}

#[test]
fn expect_ordering() {
    expect(10).to_be_greater_than(5);
    expect(10).to_be_less_than(20);
    expect(10).to_be_at_least(10);
    expect(10).to_be_at_most(10);
}

#[test]
fn expect_options_and_results() {
    expect(Some(42)).to_be_some();
    expect(None::<i32>).to_be_none();
    expect::<Result<i32, String>>(Ok(1)).to_be_ok();
    expect::<Result<i32, String>>(Err("bad".into())).to_be_err();
}

#[test]
fn expect_containers() {
    expect(vec![1, 2, 3]).to_have_length(3);
    expect(vec!["a", "b", "c"]).to_contain("b");
    expect::<Vec<i32>>(vec![]).to_be_empty();
}

dataset!(
    squares_numbers,
    [
        one      => (1, 1),
        two      => (2, 4),
        three    => (3, 9),
        negative => (-3, 9),
    ],
    |(n, sq): (i32, i32)| {
        expect(n * n).to_be(sq);
    }
);

dataset!(
    string_lengths,
    [
        short => ("hi", 2),
        med   => ("hello", 5),
        empty => ("", 0),
    ],
    |(s, len): (&str, usize)| {
        expect(s.len()).to_be(len);
    }
);

#[test]
fn json_helpers_dig_into_response() {
    // Build a TestResponse manually to exercise the assertion surface without
    // needing a full Application.
    let resp = anvil_test::TestResponse {
        status: http::StatusCode::OK,
        headers: http::HeaderMap::new(),
        body: json!({
            "data": {
                "user": { "name": "Alice", "roles": ["admin", "user"] }
            }
        })
        .to_string()
        .into_bytes(),
    };
    resp.assert_ok()
        .assert_json_path("data.user.name", json!("Alice"))
        .assert_json_path("data.user.roles.0", json!("admin"))
        .assert_json_fragment(json!({ "data": { "user": { "name": "Alice" } } }))
        .assert_see("Alice")
        .assert_dont_see("Bob");
}

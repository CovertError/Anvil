//! Fluent assertions — Pest's `expect()` translated to Rust.
//!
//! ```ignore
//! use anvilforge::assay::*;
//!
//! expect(2 + 2).to_be(4);
//! expect("hello world").to_contain("world");
//! expect(vec![1, 2, 3]).to_have_length(3);
//! expect(Some(5)).to_be_some();
//! expect(value).not().to_be(0);
//! ```
//!
//! Two top-level types: `Expect<T>` (positive form) and `Not<T>` (negated form,
//! returned by `.not()`). Both implement the same set of matchers, but the
//! `Not<T>` versions invert the assertion. Every matcher returns `self` so they
//! can chain.

use std::fmt::Debug;

pub fn expect<T>(value: T) -> Expect<T> {
    Expect(value)
}

pub struct Expect<T>(pub T);
pub struct Not<T>(pub T);

impl<T> Expect<T> {
    pub fn not(self) -> Not<T> {
        Not(self.0)
    }
    pub fn value(&self) -> &T {
        &self.0
    }
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Not<T> {
    pub fn value(&self) -> &T {
        &self.0
    }
}

// ─── Equality matchers ─────────────────────────────────────────────────────

impl<T: PartialEq + Debug> Expect<T> {
    pub fn to_be(self, expected: T) -> Self {
        assert!(
            self.0 == expected,
            "expected {:?} to equal {:?}",
            self.0,
            expected
        );
        self
    }
    pub fn to_equal(self, expected: T) -> Self {
        self.to_be(expected)
    }
}

impl<T: PartialEq + Debug> Not<T> {
    pub fn to_be(self, expected: T) -> Self {
        assert!(
            self.0 != expected,
            "expected {:?} NOT to equal {:?}",
            self.0,
            expected
        );
        self
    }
    pub fn to_equal(self, expected: T) -> Self {
        self.to_be(expected)
    }
}

// ─── Truthiness ────────────────────────────────────────────────────────────

impl Expect<bool> {
    pub fn to_be_true(self) -> Self {
        assert!(self.0, "expected true, got false");
        self
    }
    pub fn to_be_false(self) -> Self {
        assert!(!self.0, "expected false, got true");
        self
    }
}

// ─── Ordering ──────────────────────────────────────────────────────────────

impl<T: PartialOrd + Debug> Expect<T> {
    pub fn to_be_greater_than(self, other: T) -> Self {
        assert!(self.0 > other, "expected {:?} > {:?}", self.0, other);
        self
    }
    pub fn to_be_less_than(self, other: T) -> Self {
        assert!(self.0 < other, "expected {:?} < {:?}", self.0, other);
        self
    }
    pub fn to_be_at_least(self, other: T) -> Self {
        assert!(self.0 >= other, "expected {:?} >= {:?}", self.0, other);
        self
    }
    pub fn to_be_at_most(self, other: T) -> Self {
        assert!(self.0 <= other, "expected {:?} <= {:?}", self.0, other);
        self
    }
}

// ─── String contents ───────────────────────────────────────────────────────

impl Expect<&str> {
    pub fn to_contain(self, needle: &str) -> Self {
        assert!(
            self.0.contains(needle),
            "expected `{}` to contain `{}`",
            self.0,
            needle
        );
        self
    }
    pub fn to_start_with(self, prefix: &str) -> Self {
        assert!(
            self.0.starts_with(prefix),
            "expected `{}` to start with `{}`",
            self.0,
            prefix
        );
        self
    }
    pub fn to_end_with(self, suffix: &str) -> Self {
        assert!(
            self.0.ends_with(suffix),
            "expected `{}` to end with `{}`",
            self.0,
            suffix
        );
        self
    }
    pub fn to_match(self, regex: &str) -> Self {
        let re = regex::Regex::new(regex).expect("invalid regex");
        assert!(
            re.is_match(self.0),
            "expected `{}` to match /{regex}/",
            self.0
        );
        self
    }
}

impl Expect<String> {
    pub fn to_contain(self, needle: &str) -> Self {
        assert!(
            self.0.contains(needle),
            "expected `{}` to contain `{needle}`",
            self.0
        );
        self
    }
    pub fn to_start_with(self, prefix: &str) -> Self {
        assert!(
            self.0.starts_with(prefix),
            "expected `{}` to start with `{prefix}`",
            self.0
        );
        self
    }
    pub fn to_end_with(self, suffix: &str) -> Self {
        assert!(
            self.0.ends_with(suffix),
            "expected `{}` to end with `{suffix}`",
            self.0
        );
        self
    }
    pub fn to_match(self, regex: &str) -> Self {
        let re = regex::Regex::new(regex).expect("invalid regex");
        assert!(
            re.is_match(&self.0),
            "expected `{}` to match /{regex}/",
            self.0
        );
        self
    }
}

// ─── Containers ────────────────────────────────────────────────────────────

impl<T: Debug + PartialEq> Expect<Vec<T>> {
    pub fn to_have_length(self, len: usize) -> Self {
        assert_eq!(self.0.len(), len, "expected length {len}, got {}", self.0.len());
        self
    }
    pub fn to_contain(self, item: T) -> Self {
        assert!(
            self.0.iter().any(|v| v == &item),
            "expected {:?} to contain {:?}",
            self.0,
            item
        );
        self
    }
    pub fn to_be_empty(self) -> Self {
        assert!(self.0.is_empty(), "expected empty, got {:?}", self.0);
        self
    }
}

// ─── Option ────────────────────────────────────────────────────────────────

impl<T: Debug> Expect<Option<T>> {
    pub fn to_be_some(self) -> Self {
        assert!(self.0.is_some(), "expected Some(_), got None");
        self
    }
    pub fn to_be_none(self) -> Self {
        assert!(self.0.is_none(), "expected None, got {:?}", self.0);
        self
    }
}

// ─── Result ────────────────────────────────────────────────────────────────

impl<T: Debug, E: Debug> Expect<Result<T, E>> {
    pub fn to_be_ok(self) -> Self {
        assert!(self.0.is_ok(), "expected Ok, got {:?}", self.0);
        self
    }
    pub fn to_be_err(self) -> Self {
        assert!(self.0.is_err(), "expected Err, got {:?}", self.0);
        self
    }
}

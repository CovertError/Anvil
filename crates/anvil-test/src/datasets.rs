//! Pest-style parameterized tests via the `dataset!` macro.
//!
//! Pest in Laravel-land lets you write:
//!
//! ```php
//! it('squares numbers', function ($n, $sq) {
//!     expect($n * $n)->toBe($sq);
//! })->with([[1, 1], [2, 4], [3, 9]]);
//! ```
//!
//! Rust's `#[test]` is rigid about test names and arguments, so we generate
//! a separate `#[tokio::test]` per row. The `dataset!` macro takes a base test
//! name, a list of named cases, the parameter list, and the test body.
//!
//! ```ignore
//! use anvilforge::assay::*;
//!
//! dataset!(squares_numbers, [
//!     one      => (1, 1),
//!     two      => (2, 4),
//!     three    => (3, 9),
//!     negative => (-3, 9),
//! ], |(n, sq): (i32, i32)| {
//!     expect(n * n).to_be(sq);
//! });
//! ```
//!
//! The closure parameter is a single tuple pattern + tuple type. Rust's
//! declarative macros can't cleanly mix the case-level repetition with a
//! per-arg parameter list, so we destructure once per generated test.
//!
//! Each row becomes its own test:
//!
//!   - `squares_numbers__one`
//!   - `squares_numbers__two`
//!   - `squares_numbers__three`
//!   - `squares_numbers__negative`
//!
//! Async variants are supported by using `dataset!(name, [...], async |...| { ... })`.

/// Generate one `#[test]` per row. Each row is `case_name => (arg1, arg2, ...)`.
/// The closure parameter is a single tuple pattern + tuple type:
/// `|(n, sq): (i32, i32)|`. For one-arg cases use `|(n,): (i32,)|`.
#[macro_export]
macro_rules! dataset {
    ($base:ident, [ $($case:ident => $args:tt),* $(,)? ], | $pat:tt : $ty:tt | $body:block) => {
        $(
            $crate::paste::paste! {
                #[test]
                fn [<$base __ $case>]() {
                    let $pat: $ty = $args;
                    $body
                }
            }
        )*
    };
}

/// Async variant — generates `#[tokio::test]` per row.
#[macro_export]
macro_rules! dataset_async {
    ($base:ident, [ $($case:ident => $args:tt),* $(,)? ], async | $pat:tt : $ty:tt | $body:block) => {
        $(
            $crate::paste::paste! {
                #[tokio::test]
                async fn [<$base __ $case>]() {
                    let $pat: $ty = $args;
                    $body
                }
            }
        )*
    };
}

//! Pagination — mirrors Laravel's `LengthAwarePaginator`.

use serde::Serialize;

/// Page-aware paginator. Produced by `QueryBuilder::paginate(per_page, page, pool)`.
#[derive(Debug, Clone, Serialize)]
pub struct Paginator<T> {
    /// The rows on the current page.
    pub items: Vec<T>,
    /// Total row count across all pages (a `COUNT(*)` against the same query).
    pub total: i64,
    /// Rows per page.
    pub per_page: u64,
    /// 1-indexed current page.
    pub current_page: u64,
    /// 1-indexed last page (computed from `total` + `per_page`).
    pub last_page: u64,
}

impl<T> Paginator<T> {
    /// Build a paginator from a fetched page + the global count.
    pub fn new(items: Vec<T>, total: i64, per_page: u64, current_page: u64) -> Self {
        let per_page = per_page.max(1);
        let total_u = total.max(0) as u64;
        let last_page = if total_u == 0 {
            1
        } else {
            total_u.div_ceil(per_page)
        };
        Self {
            items,
            total,
            per_page,
            current_page: current_page.max(1),
            last_page,
        }
    }

    pub fn has_more_pages(&self) -> bool {
        self.current_page < self.last_page
    }

    pub fn has_previous_pages(&self) -> bool {
        self.current_page > 1
    }

    pub fn next_page(&self) -> Option<u64> {
        self.has_more_pages().then(|| self.current_page + 1)
    }

    pub fn previous_page(&self) -> Option<u64> {
        self.has_previous_pages().then(|| self.current_page - 1)
    }

    /// Number of items on the current page.
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// `from` and `to` are 1-indexed row positions within the *full* result set.
    pub fn from(&self) -> Option<u64> {
        if self.items.is_empty() {
            None
        } else {
            Some((self.current_page - 1) * self.per_page + 1)
        }
    }

    pub fn to(&self) -> Option<u64> {
        if self.items.is_empty() {
            None
        } else {
            Some((self.current_page - 1) * self.per_page + self.items.len() as u64)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Map items via a function while keeping the pagination metadata.
    pub fn map<U, F: FnMut(T) -> U>(self, mut f: F) -> Paginator<U> {
        Paginator {
            items: self.items.into_iter().map(&mut f).collect(),
            total: self.total,
            per_page: self.per_page,
            current_page: self.current_page,
            last_page: self.last_page,
        }
    }
}

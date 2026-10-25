//! Success response shapes (D-035): one resource is `{"data": ...}`, a list is
//! `{"data": [...], "next_cursor": ...}`. The request id travels in the `x-request-id` header.

use serde::Serialize;

/// One resource.
#[derive(Debug, Serialize)]
pub(crate) struct Data<T> {
    pub(crate) data: T,
}

/// A page of a list; `next_cursor` is `null` on the last page.
#[derive(Debug, Serialize)]
pub(crate) struct Page<T> {
    pub(crate) data: Vec<T>,
    pub(crate) next_cursor: Option<String>,
}

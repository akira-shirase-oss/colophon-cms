//! `POST /api/v1/rich-text/validate`: the server's verdict on a rich-text document, so the
//! Studio, the testbed and scripts can check a document before saving it. Saving runs the same
//! validator; this endpoint never stores anything.
//!
//! It needs no sign-in: it reads nothing and stores nothing, so it answers anyone the same way.

use axum::{
    Json,
    extract::rejection::JsonRejection,
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use serde_json::Value;

use crate::ApiError;

/// 200 with `{"value": <canonical document>, "nodes", "depth", "bytes"}`, or 422 with the
/// error envelope, whose `details` name the rejection code and its JSON Pointer.
pub(crate) async fn validate(
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(value) = body?;
    let document =
        colophon_domain::rich_text::validate(&value).map_err(ApiError::InvalidRichText)?;
    // The canonical JSON is embedded as is: re-serializing it could reorder its keys.
    let body = format!(
        r#"{{"value":{},"nodes":{},"depth":{},"bytes":{}}}"#,
        document.canonical_json(),
        document.nodes(),
        document.depth(),
        document.bytes()
    );
    let mut response = body.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    Ok(response)
}

//! A query document and a cursor from any bytes: refused or accepted, never a panic. The input splits at
//! its first NUL: a JSON query before it, a cursor token after.
#![no_main]

mod common;

use colophon_domain::query::Query;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let (document, cursor) = match data.iter().position(|byte| *byte == 0) {
        Some(at) => (&data[..at], &data[at + 1..]),
        None => (data, &[][..]),
    };
    let posts = common::posts();
    let query = serde_json::from_slice::<serde_json::Value>(document)
        .ok()
        .and_then(|value| Query::validate(&posts, &value).ok());
    if let (Some(query), Ok(token)) = (query, std::str::from_utf8(cursor)) {
        let _ = query.after_cursor(token);
    }
});

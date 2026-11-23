//! Rich text from any bytes: refused or accepted, never a panic; and what is accepted is a fixed point,
//! its canonical form accepted again unchanged (the editor and the server agree byte for byte).
#![no_main]

use colophon_domain::rich_text;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(data) else {
        return;
    };
    if let Ok(document) = rich_text::validate(&value) {
        let again: serde_json::Value =
            serde_json::from_str(document.canonical_json()).expect("canonical JSON parses");
        let twice = rich_text::validate(&again).expect("the canonical form is accepted");
        assert_eq!(twice.canonical_json(), document.canonical_json(), "not a fixed point");
    }
});

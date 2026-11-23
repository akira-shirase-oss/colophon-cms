//! The small text parsers outside input reaches: links, locales, receipt markers, preview templates,
//! destination addresses, visibilities and schedule times. The input's first byte picks one; the rest is
//! its text. Each answers, never panics, and keeps its promise: a filled preview address stays on its
//! template's origin, a canonical locale is a fixed point, a visibility round-trips.
#![no_main]

use colophon_domain::{
    audience::Visibility,
    destination, locale,
    preview::{Fill, Template},
    receipt, rich_text,
    schedule::{self, Choice},
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&pick, rest)) = data.split_first() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(rest) else {
        return;
    };
    match pick % 7 {
        0 => {
            let _ = rich_text::is_allowed_href(text);
        }
        1 => {
            let once = locale::canonical(text);
            assert_eq!(locale::canonical(&once), once, "canonical locale not a fixed point");
        }
        2 => {
            let _ = receipt::marker(text);
        }
        3 => {
            let (template, value) = text.split_once('\n').unwrap_or((text, "x"));
            if let Ok(parsed) = Template::parse(template, pick & 0x80 != 0) {
                let filled = parsed.fill(&Fill {
                    code: value,
                    collection: value,
                    id: value,
                    slug: value,
                    locale: value,
                });
                let url = url::Url::parse(&filled).expect("a filled template is a URL");
                assert_eq!(url.origin().ascii_serialization(), parsed.origin(), "left its origin");
            }
        }
        4 => {
            let _ = destination::check_url(text, pick & 0x80 != 0);
        }
        5 => {
            if let Some(visibility) = Visibility::parse(text) {
                assert_eq!(Visibility::parse(&visibility.value()), Some(visibility));
            }
        }
        _ => {
            let (local, zone) = text.split_once('\n').unwrap_or((text, "UTC"));
            let choice = match pick & 0xc0 {
                0x40 => Some(Choice::Earlier),
                0x80 => Some(Choice::Later),
                _ => None,
            };
            let _ = schedule::resolve(local, zone, choice);
        }
    }
});

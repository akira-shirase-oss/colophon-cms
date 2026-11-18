//! Locale tags (D-087, D-212). A tag names the language an entry is written in: `en`, `pt-BR`,
//! `zh-Hant-TW`. BCP 47 tags are case-insensitive (RFC 5646 §2.1.1), so `fr` and `FR` name one
//! language, and one localization group may hold only one entry for it. Every tag the API takes is
//! stored and compared in one form, the case BCP 47 recommends:
//!
//! - the language subtag lowercase (`fr`);
//! - a four-letter script subtag titlecase (`Hant`);
//! - a two-letter region subtag uppercase (`BR`), a three-digit region as it is (`419`);
//! - every other subtag lowercase: variants, and everything after a singleton (an extension such
//!   as `u-ca-buddhist`, or private use such as `x-legal`).
//!
//! Only case changes: no alias is mapped (`iw` stays `iw`, not `he`) and no subtag is reordered,
//! so a tag reads back as it was written, apart from its letters' case. The database's
//! `colophon_canonical_locale` (migration 0077) applies the same rule in a trigger to every tag
//! written, so the group index refuses a case-only duplicate even if a path missed this.

/// `tag` in BCP 47's recommended case (see the module). Assumes a tag already shape-checked
/// (ASCII letters, digits and `-`); anything else is returned with only its ASCII letters recased.
#[must_use]
pub fn canonical(tag: &str) -> String {
    let mut out = String::with_capacity(tag.len());
    let mut after_singleton = false;
    for (index, sub) in tag.split('-').enumerate() {
        if index > 0 {
            out.push('-');
        }
        let letters = !sub.is_empty() && sub.bytes().all(|byte| byte.is_ascii_alphabetic());
        if index == 0 || after_singleton {
            out.push_str(&sub.to_ascii_lowercase());
        } else if sub.len() == 1 {
            // A singleton starts an extension or private use: it and all after it are lowercase.
            after_singleton = true;
            out.push_str(&sub.to_ascii_lowercase());
        } else if letters && sub.len() == 4 {
            let lower = sub.to_ascii_lowercase();
            let mut chars = lower.chars();
            if let Some(first) = chars.next() {
                out.push(first.to_ascii_uppercase());
                out.push_str(chars.as_str());
            }
        } else if letters && sub.len() == 2 {
            out.push_str(&sub.to_ascii_uppercase());
        } else {
            out.push_str(&sub.to_ascii_lowercase());
        }
    }
    out
}

/// Whether two tags name the same locale: equal once canonical (case only, D-212).
#[must_use]
pub fn same(left: &str, right: &str) -> bool {
    canonical(left) == canonical(right)
}

#[cfg(test)]
mod tests {
    use super::{canonical, same};

    /// Every upper/lower spelling of `tag` (its letters each either case): 2^letters variants.
    fn spellings(tag: &str) -> Vec<String> {
        let letters: Vec<usize> = tag
            .char_indices()
            .filter(|(_, c)| c.is_ascii_alphabetic())
            .map(|(at, _)| at)
            .collect();
        let count = letters.len().min(12);
        (0..1_u32 << count)
            .map(|mask| {
                let mut bytes = tag.to_ascii_lowercase().into_bytes();
                for (bit, &at) in letters.iter().take(count).enumerate() {
                    if mask & (1 << bit) != 0 {
                        bytes[at] = bytes[at].to_ascii_uppercase();
                    }
                }
                String::from_utf8(bytes).expect("ASCII stays UTF-8")
            })
            .collect()
    }

    const TAGS: &[(&str, &str)] = &[
        ("en", "en"),
        ("fr", "fr"),
        ("pt-br", "pt-BR"),
        ("zh-hant-tw", "zh-Hant-TW"),
        ("sr-latn", "sr-Latn"),
        ("es-419", "es-419"),
        ("de-ch-1901", "de-CH-1901"),
        ("sl-rozaj-biske", "sl-rozaj-biske"),
        ("en-us-u-ca-gregory", "en-US-u-ca-gregory"),
        ("en-x-legal-ab", "en-x-legal-ab"),
        ("iw", "iw"),
        ("yue-hk", "yue-HK"),
    ];

    #[test]
    fn every_case_spelling_of_a_tag_has_one_canonical_form() {
        for (tag, expected) in TAGS {
            for spelling in spellings(tag) {
                assert_eq!(canonical(&spelling), *expected, "{spelling}");
                assert!(same(&spelling, expected), "{spelling}");
            }
        }
    }

    #[test]
    fn canonical_is_idempotent() {
        for (tag, _) in TAGS {
            for spelling in spellings(tag) {
                let once = canonical(&spelling);
                assert_eq!(canonical(&once), once, "{spelling}");
            }
        }
    }

    #[test]
    fn only_case_changes_never_the_subtags() {
        for (tag, _) in TAGS {
            for spelling in spellings(tag) {
                assert!(canonical(&spelling).eq_ignore_ascii_case(&spelling), "{spelling}");
            }
        }
        // No alias mapping: `iw` is not rewritten to `he`.
        assert_eq!(canonical("IW"), "iw");
        assert!(!same("iw", "he"));
    }

    #[test]
    fn different_languages_stay_different() {
        assert!(!same("fr", "fr-CA"));
        assert!(!same("pt-BR", "pt-PT"));
        assert!(!same("zh-Hant", "zh-Hans"));
    }
}

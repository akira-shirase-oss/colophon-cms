//! Languages (docs/modules/06-delivery.md, "Locales"; D-087, D-180, D-181): the shape of a
//! locale tag, an environment's language list with its default and ordered fallbacks, the
//! chain a delivery request resolves through, and the resolution itself — which published
//! member of a translation group a reader gets.
//!
//! An entry with no tag is written in the environment's default language, so for resolution
//! it counts as the default tag; with no default configured it matches nothing a reader can
//! ask for by name. Tags compare without case, as BCP 47 does.

use sha2::{Digest, Sha256};

/// The most languages one environment lists.
pub const MAX_LOCALES: usize = 50;
/// The most fallbacks one language names, and the most a request may name in `?fallback=`.
pub const MAX_FALLBACKS: usize = 4;
/// The longest label.
pub const MAX_LABEL: usize = 80;

/// Whether `tag` has a BCP-47 tag's shape: a 2–3 letter primary subtag, then subtags of one to
/// eight letters or digits, 35 characters in all (BCP 47's own bound). Shape only — Colophon
/// keeps no registry of languages.
pub fn is_tag(tag: &str) -> bool {
    tag.len() <= 35
        && tag.split('-').enumerate().all(|(index, part)| {
            if index == 0 {
                (2..=3).contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_alphabetic())
            } else {
                (1..=8).contains(&part.len())
                    && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
            }
        })
}

/// One language in an environment's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Language {
    /// Its tag, as the list spells it.
    pub tag: String,
    /// What people call it.
    pub label: String,
    /// Whether it is the environment's default: the language of entries with no tag.
    pub default: bool,
    /// The listed tags to serve, in order, when this one isn't published.
    pub fallback: Vec<String>,
}

/// Why a language list was refused: the code clients branch on, where, and what to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyRefusal {
    /// `invalid_locale`, `invalid_locale_label`, `duplicate_locale`, `too_many_locales`,
    /// `locale_default_required`, `fallback_not_listed` or `invalid_fallback`.
    pub code: &'static str,
    /// RFC 6901 pointer into the request body (`/locales/2/fallback/0`).
    pub pointer: String,
    /// For people.
    pub message: String,
}

fn refused(code: &'static str, pointer: String, message: impl Into<String>) -> PolicyRefusal {
    PolicyRefusal {
        code,
        pointer,
        message: message.into(),
    }
}

/// Checks a whole language list, as `PUT {P}/locales` replaces it: at most [`MAX_LOCALES`]
/// well-formed tags, no two the same without case, labels of 1–[`MAX_LABEL`] characters
/// (trimmed), exactly one default when the list isn't empty, and each language's fallbacks —
/// at most [`MAX_FALLBACKS`], never itself, never repeated — drawn from the list. Labels are
/// trimmed in place; fallbacks are respelled as the list spells them. The first problem wins,
/// in list order.
pub fn check_policy(languages: &mut [Language]) -> Result<(), PolicyRefusal> {
    if languages.len() > MAX_LOCALES {
        return Err(refused(
            "too_many_locales",
            "/locales".to_owned(),
            format!("an environment lists at most {MAX_LOCALES} languages"),
        ));
    }
    for (index, language) in languages.iter_mut().enumerate() {
        if !is_tag(&language.tag) {
            return Err(refused(
                "invalid_locale",
                format!("/locales/{index}/tag"),
                format!(
                    "\"{}\" isn't a BCP-47 language tag (like \"en\" or \"pt-BR\")",
                    language.tag
                ),
            ));
        }
        let label = language.label.trim();
        if label.is_empty() || label.chars().count() > MAX_LABEL {
            return Err(refused(
                "invalid_locale_label",
                format!("/locales/{index}/label"),
                format!("a language's name is 1 to {MAX_LABEL} characters"),
            ));
        }
        language.label = label.to_owned();
    }
    for (index, language) in languages.iter().enumerate() {
        if languages[..index]
            .iter()
            .any(|earlier| earlier.tag.eq_ignore_ascii_case(&language.tag))
        {
            return Err(refused(
                "duplicate_locale",
                format!("/locales/{index}/tag"),
                format!("{} is listed twice", language.tag),
            ));
        }
    }
    let defaults = languages.iter().filter(|language| language.default).count();
    if !languages.is_empty() && defaults != 1 {
        return Err(refused(
            "locale_default_required",
            "/locales".to_owned(),
            "choose exactly one default language",
        ));
    }
    let listed: Vec<String> = languages
        .iter()
        .map(|language| language.tag.clone())
        .collect();
    for (index, language) in languages.iter_mut().enumerate() {
        if language.fallback.len() > MAX_FALLBACKS {
            return Err(refused(
                "invalid_fallback",
                format!("/locales/{index}/fallback"),
                format!("a language falls back to at most {MAX_FALLBACKS} others"),
            ));
        }
        let mut spelled: Vec<String> = Vec::with_capacity(language.fallback.len());
        for (position, tag) in language.fallback.iter().enumerate() {
            let Some(listed) = listed
                .iter()
                .find(|listed| listed.eq_ignore_ascii_case(tag))
            else {
                return Err(refused(
                    "fallback_not_listed",
                    format!("/locales/{index}/fallback/{position}"),
                    format!("{tag} isn't one of this environment's languages; add it first"),
                ));
            };
            if listed.eq_ignore_ascii_case(&language.tag)
                || spelled.iter().any(|earlier| earlier == listed)
            {
                return Err(refused(
                    "invalid_fallback",
                    format!("/locales/{index}/fallback"),
                    format!(
                        "{}'s fallbacks name {listed} twice or name the language itself",
                        language.tag
                    ),
                ));
            }
            spelled.push(listed.clone());
        }
        language.fallback = spelled;
    }
    Ok(())
}

/// What `?fallback=` asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fallback {
    /// The environment's own chain for the tag (the default).
    Policy,
    /// The tag alone.
    None,
    /// These tags, in this order (at most [`MAX_FALLBACKS`]).
    Tags(Vec<String>),
}

impl Fallback {
    /// Parses `?fallback=`: absent is [`Fallback::Policy`]; `policy`, `none`, or up to
    /// [`MAX_FALLBACKS`] comma-separated well-formed tags. `None` when it is none of those.
    pub fn parse(text: Option<&str>) -> Option<Self> {
        match text {
            None | Some("policy") => Some(Self::Policy),
            Some("none") => Some(Self::None),
            Some(list) => {
                let tags: Vec<String> = list.split(',').map(str::trim).map(str::to_owned).collect();
                let fits = !tags.is_empty()
                    && tags.len() <= MAX_FALLBACKS
                    && tags.iter().all(|tag| is_tag(tag));
                fits.then_some(Self::Tags(tags))
            }
        }
    }

    /// Whether the request named its fallbacks itself, rather than taking the policy's.
    pub const fn explicit(&self) -> bool {
        !matches!(self, Self::Policy)
    }
}

/// The tags a request resolves through, best first: the requested tag, then its fallbacks —
/// the policy's chain, none, or the request's own — with repeats (without case) dropped.
pub fn chain(requested: &str, fallback: &Fallback, policy: &[String]) -> Vec<String> {
    let mut tags = vec![requested.to_owned()];
    let rest: &[String] = match fallback {
        Fallback::Policy => policy,
        Fallback::None => &[],
        Fallback::Tags(tags) => tags,
    };
    for tag in rest {
        if !tags.iter().any(|kept| kept.eq_ignore_ascii_case(tag)) {
            tags.push(tag.clone());
        }
    }
    tags
}

/// A fixed-width name for a chain, for cache keys: the first 16 bytes of SHA-256 over its
/// tags joined with commas. The requested tag is the chain's first, so the digest names it
/// too. Spelling counts — delivery matches tags exactly — so requests are respelled as the
/// environment's list has them before they get here.
pub fn chain_digest(chain: &[String]) -> u128 {
    let joined = chain.join(",");
    let digest = Sha256::digest(joined.as_bytes());
    let mut first = [0_u8; 16];
    first.copy_from_slice(&digest[..16]);
    u128::from_be_bytes(first)
}

/// The tag a stored locale reads as: its own, or — for an entry with none — the default.
pub fn effective<'a>(locale: Option<&'a str>, default: Option<&'a str>) -> Option<&'a str> {
    locale.or(default)
}

/// Which of a translation group's published members a reader gets: the index into
/// `candidates` (each member's stored tag, `None` for an untagged entry) of the first match for
/// the earliest tag in `chain` — an untagged member matches the default — and whether that
/// is a fallback (not the chain's first tag). `None` when no member matches any tag. Ties
/// within one tag go to the earliest candidate. Tags match exactly, as delivery's reads do.
pub fn resolve(
    chain: &[String],
    default: Option<&str>,
    candidates: &[Option<&str>],
) -> Option<(usize, bool)> {
    chain.iter().enumerate().find_map(|(rank, wanted)| {
        candidates
            .iter()
            .position(|stored| effective(*stored, default).is_some_and(|tag| tag == wanted))
            .map(|index| (index, rank > 0))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn language(tag: &str, default: bool, fallback: &[&str]) -> Language {
        Language {
            tag: tag.to_owned(),
            label: format!("  {tag} label "),
            default,
            fallback: fallback.iter().map(|tag| (*tag).to_owned()).collect(),
        }
    }

    fn tags(list: &[&str]) -> Vec<String> {
        list.iter().map(|tag| (*tag).to_owned()).collect()
    }

    #[test]
    fn a_tag_has_bcp_47s_shape() {
        for good in ["en", "pt-BR", "zh-Hant-TW", "sr-Latn", "es-419", "ast"] {
            assert!(is_tag(good), "{good}");
        }
        for bad in [
            "",
            "e",
            "english",
            "en_US",
            "en-",
            "-en",
            "en--US",
            "pt-BRAZILIAN1",
            "1n",
        ] {
            assert!(!is_tag(bad), "{bad}");
        }
        assert!(!is_tag(&format!("en-{}", "abcdefgh-".repeat(4))), "over 35");
    }

    #[test]
    fn a_list_needs_one_default_listed_fallbacks_and_unique_tags() {
        let mut good = vec![
            language("en", true, &[]),
            language("pt", false, &["EN"]),
            language("pt-BR", false, &["pt", "en"]),
        ];
        assert_eq!(check_policy(&mut good), Ok(()));
        assert_eq!(good[0].label, "en label", "trimmed");
        assert_eq!(good[1].fallback, ["en"], "respelled as listed");
        assert_eq!(check_policy(&mut []), Ok(()), "no list: one language");

        let code =
            |mut list: Vec<Language>| check_policy(&mut list).map_err(|r| (r.code, r.pointer));
        assert_eq!(
            code(vec![language("en", false, &[])]),
            Err(("locale_default_required", "/locales".to_owned()))
        );
        assert_eq!(
            code(vec![language("en", true, &[]), language("fr", true, &[])]),
            Err(("locale_default_required", "/locales".to_owned()))
        );
        assert_eq!(
            code(vec![language("en", true, &[]), language("EN", false, &[])]),
            Err(("duplicate_locale", "/locales/1/tag".to_owned()))
        );
        assert_eq!(
            code(vec![
                language("en", true, &[]),
                language("fr", false, &["de"])
            ]),
            Err(("fallback_not_listed", "/locales/1/fallback/0".to_owned()))
        );
        assert_eq!(
            code(vec![language("en", true, &["en"])]),
            Err(("invalid_fallback", "/locales/0/fallback".to_owned()))
        );
        assert_eq!(
            code(vec![
                language("en", true, &[]),
                language("fr", false, &["en", "EN"])
            ]),
            Err(("invalid_fallback", "/locales/1/fallback".to_owned()))
        );
        assert_eq!(
            code(vec![language("en", true, &["a1", "b2", "c3", "d4", "e5"])]),
            Err(("invalid_fallback", "/locales/0/fallback".to_owned()))
        );
        assert_eq!(
            code(vec![language("english", true, &[])]),
            Err(("invalid_locale", "/locales/0/tag".to_owned()))
        );
        let mut blank = language("en", true, &[]);
        blank.label = "   ".to_owned();
        assert_eq!(
            code(vec![blank]),
            Err(("invalid_locale_label", "/locales/0/label".to_owned()))
        );
        let many: Vec<Language> = (0..=MAX_LOCALES)
            .map(|n| language(&format!("xx-{n}"), n == 0, &[]))
            .collect();
        assert_eq!(code(many), Err(("too_many_locales", "/locales".to_owned())));
    }

    #[test]
    fn fallback_parses_policy_none_or_a_short_list() {
        assert_eq!(Fallback::parse(None), Some(Fallback::Policy));
        assert_eq!(Fallback::parse(Some("policy")), Some(Fallback::Policy));
        assert_eq!(Fallback::parse(Some("none")), Some(Fallback::None));
        assert_eq!(
            Fallback::parse(Some("pt,en")),
            Some(Fallback::Tags(tags(&["pt", "en"])))
        );
        for bad in ["", "pt,,en", "english", "a1,b2,c3,d4,e5"] {
            assert_eq!(Fallback::parse(Some(bad)), None, "{bad}");
        }
        assert!(!Fallback::Policy.explicit());
        assert!(Fallback::None.explicit());
    }

    #[test]
    fn a_chain_is_the_tag_then_its_fallbacks_once_each() {
        let policy = tags(&["pt", "en"]);
        assert_eq!(
            chain("pt-BR", &Fallback::Policy, &policy),
            ["pt-BR", "pt", "en"]
        );
        assert_eq!(chain("pt-BR", &Fallback::None, &policy), ["pt-BR"]);
        assert_eq!(
            chain(
                "pt-BR",
                &Fallback::Tags(tags(&["en", "PT-br", "en"])),
                &policy
            ),
            ["pt-BR", "en"]
        );
        assert_ne!(
            chain_digest(&chain("pt-BR", &Fallback::Policy, &policy)),
            chain_digest(&chain("pt-BR", &Fallback::None, &policy))
        );
        assert_ne!(
            chain_digest(&tags(&["pt-BR", "en"])),
            chain_digest(&tags(&["pt-br", "en"])),
            "delivery matches spelling exactly, so spelling is part of the key"
        );
    }

    #[test]
    fn resolution_takes_the_earliest_tag_that_is_published() {
        let chain = tags(&["pt-BR", "pt", "en"]);
        assert_eq!(
            resolve(&chain, Some("en"), &[Some("pt"), Some("pt-BR")]),
            Some((1, false))
        );
        assert_eq!(
            resolve(&chain, Some("en"), &[Some("fr"), Some("pt")]),
            Some((1, true))
        );
        assert_eq!(
            resolve(&chain, Some("en"), &[Some("fr"), None]),
            Some((1, true)),
            "an untagged entry is the default language"
        );
        assert_eq!(
            resolve(&chain, None, &[Some("fr"), None]),
            None,
            "no default: no match"
        );
        assert_eq!(resolve(&chain, Some("en"), &[]), None);
        assert_eq!(
            resolve(&tags(&["PT-br"]), None, &[Some("pt-BR")]),
            None,
            "spelled exactly, as delivery's reads match"
        );
    }

    /// Every chain over a small alphabet, against every set of candidates (each tag or none,
    /// with and without a default): the answer is always in the chain, never a later tag when
    /// an earlier one is published, a fallback exactly when it isn't the first, and with a
    /// one-tag chain an exact match or nothing.
    #[test]
    fn resolution_holds_its_properties_over_every_small_case() {
        let alphabet = ["en", "fr", "de", "pt"];
        let mut chains: Vec<Vec<String>> = Vec::new();
        for a in alphabet {
            chains.push(tags(&[a]));
            for b in alphabet.iter().filter(|b| **b != a) {
                chains.push(tags(&[a, b]));
                for c in alphabet.iter().filter(|c| **c != a && *c != b) {
                    chains.push(tags(&[a, b, c]));
                }
            }
        }
        let options: [Option<&str>; 5] = [None, Some("en"), Some("fr"), Some("de"), Some("pt")];
        let mut cases = 0;
        for chain in &chains {
            for default in [None, Some("en"), Some("de")] {
                for mask in 0_u32..(1 << options.len()) {
                    let candidates: Vec<Option<&str>> = options
                        .iter()
                        .enumerate()
                        .filter(|(bit, _)| mask & (1 << bit) != 0)
                        .map(|(_, option)| *option)
                        .collect();
                    cases += 1;
                    let rank_of = |stored: Option<&str>| {
                        effective(stored, default)
                            .and_then(|tag| chain.iter().position(|wanted| wanted == tag))
                    };
                    let best = candidates
                        .iter()
                        .filter_map(|stored| rank_of(*stored))
                        .min();
                    match resolve(chain, default, &candidates) {
                        None => assert_eq!(best, None, "{chain:?} {default:?} {candidates:?}"),
                        Some((index, fallback)) => {
                            let rank = rank_of(candidates[index]).expect("inside the chain");
                            assert_eq!(Some(rank), best, "never a later tag than published");
                            assert_eq!(fallback, rank > 0);
                            if chain.len() == 1 {
                                assert!(!fallback, "a one-tag chain is an exact match");
                            }
                        }
                    }
                }
            }
        }
        assert!(cases > 1000, "{cases}");
    }
}

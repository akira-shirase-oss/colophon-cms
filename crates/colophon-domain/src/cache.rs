//! Delivery cache keys and tags (docs/modules/06-delivery.md, "Cache keys" and "CDN headers and
//! purge"; spec §15; D-048). Shared by the delivery API and, later, the site build adapter, so both
//! name an answer the same way.
//!
//! A key names everything an answer depends on: the project, the environment, the recovery epoch,
//! the release generation, the render version and the normalized query. Nothing else in a request
//! changes the answer: unknown query parameters are ignored, and a cursor is reduced to the
//! generation and position it carries. A generation's published content never changes, so an
//! entry under one of these keys can't go stale; a new release is a new generation and so new
//! keys. Locales arrive later; until then every answer is in the one default locale.
//!
//! There is one exception: unpublishing (D-049) takes content out of every earlier generation's
//! answers too. So a key also names the environment's latest unpublish, and each unpublish
//! retires every answer cached before it, whatever generation it was read at (F07).
//!
//! Tags name what an answer was built from, so a purge can find it: the project, the environment,
//! the collection (a listing is tagged with its collection even when a newly published entry
//! wasn't in it, F06) and, for a single entry, the entry.

/// Changes whenever the shape of a delivery answer does, so an entry cached for the old shape is
/// never served as the new one.
pub const RENDER_VERSION: u32 = 1;

/// The most tags one purge names. A run of releases that changed more purges the whole
/// environment instead: coarser, and still bounded.
pub const MAX_PURGE_TAGS: usize = 30;

/// The tags to purge after releases of `project`'s `environment` changed `entries` (collection,
/// entry): each entry's tag, which reaches every generation's answer for it, and each
/// collection's, which reaches every listing that held it (F06, F07). More than
/// [`MAX_PURGE_TAGS`] and it is the environment's tag alone. Empty when nothing changed.
pub fn purge_tags(project: u128, environment: u128, entries: &[(String, u128)]) -> Vec<String> {
    // Tags never depend on the generation: any scope of the environment names them.
    let scope = Scope {
        project,
        environment,
        epoch: 0,
        generation: 0,
        unpublished: 0,
    };
    let mut tags: Vec<String> = Vec::new();
    for (collection, entry) in entries {
        for tag in scope.entry_tags(collection, *entry).into_iter().skip(2) {
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
    }
    if tags.len() > MAX_PURGE_TAGS {
        return vec![scope.environment_tag()];
    }
    tags.sort();
    tags
}

/// Where an answer was read: everything a key or tag starts from. Ids are UUIDs as 128-bit
/// numbers, written as fixed-width hex, so the only free text in a key is the collection name,
/// which comes last and so can't be mistaken for another field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scope {
    /// The project's id.
    pub project: u128,
    /// The environment's id (not its name: names are looked up, ids are what's stored).
    pub environment: u128,
    /// The environment's recovery epoch: an operator restore (P4) starts a new one, and
    /// generations from different epochs are never interchangeable (DL4).
    pub epoch: i64,
    /// The release generation the answer was read at.
    pub generation: i64,
    /// The generation of the environment's latest release that unpublished something (0 if none
    /// has).
    pub unpublished: i64,
}

impl Scope {
    fn prefix(&self) -> String {
        format!(
            "colophon:delivery:r{RENDER_VERSION}:p{:032x}:e{:032x}:x{}:g{}:u{}",
            self.project, self.environment, self.epoch, self.generation, self.unpublished
        )
    }

    /// The key for a page of `collection`: at most `limit` entries, after the entry `after` (a
    /// cursor's position) or from the start.
    pub fn list_key(&self, collection: &str, limit: i64, after: Option<u128>) -> String {
        let after = after.map_or_else(|| "-".to_owned(), |entry| format!("{entry:032x}"));
        format!("{}:list:{limit}:{after}:{collection}", self.prefix())
    }

    /// The key for one entry of `collection`.
    pub fn entry_key(&self, collection: &str, entry: u128) -> String {
        format!("{}:entry:{entry:032x}:{collection}", self.prefix())
    }

    /// The tags for a page of `collection`.
    pub fn list_tags(&self, collection: &str) -> Vec<String> {
        let mut tags = vec![self.project_tag(), self.environment_tag()];
        tags.extend(self.collection_tag(collection));
        tags
    }

    /// The tags for one entry of `collection`.
    pub fn entry_tags(&self, collection: &str, entry: u128) -> Vec<String> {
        let mut tags = self.list_tags(collection);
        tags.push(format!("{}:n:{entry:032x}", self.environment_tag()));
        tags
    }

    fn project_tag(&self) -> String {
        format!("colophon:p:{:032x}", self.project)
    }

    fn environment_tag(&self) -> String {
        format!("{}:e:{:032x}", self.project_tag(), self.environment)
    }

    /// Only a name that could be a collection gets a tag of its own: tags go into a response
    /// header, and a made-up name in a URL mustn't put arbitrary text there.
    fn collection_tag(&self, collection: &str) -> Option<String> {
        let bytes = collection.as_bytes();
        let plausible = (1..=64).contains(&bytes.len())
            && bytes
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_');
        plausible.then(|| format!("{}:c:{collection}", self.environment_tag()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn scope(generation: i64) -> Scope {
        Scope {
            project: 0x01,
            environment: 0x0e,
            epoch: 1,
            generation,
            unpublished: 0,
        }
    }

    #[test]
    fn a_key_names_the_generation_and_every_part_of_the_query() {
        let at_3 = scope(3);
        let first = at_3.list_key("articles", 20, None);
        assert_eq!(
            first,
            format!(
                "colophon:delivery:r1:p{:032x}:e{:032x}:x1:g3:u0:list:20:-:articles",
                1, 14
            )
        );
        assert_ne!(
            first,
            scope(4).list_key("articles", 20, None),
            "a new release, new keys"
        );
        let restored = Scope { epoch: 2, ..at_3 };
        assert_ne!(
            first,
            restored.list_key("articles", 20, None),
            "generation 3 of another epoch"
        );
        let after_unpublish = Scope {
            unpublished: 5,
            ..at_3
        };
        assert_ne!(
            first,
            after_unpublish.list_key("articles", 20, None),
            "generation 3 read again after release 5 took something down"
        );
        assert_ne!(first, at_3.list_key("articles", 21, None));
        assert_ne!(first, at_3.list_key("articles", 20, Some(7)));
        assert_ne!(first, at_3.list_key("events", 20, None));
        let other_project = Scope { project: 2, ..at_3 };
        assert_ne!(
            first,
            other_project.list_key("articles", 20, None),
            "the same collection in two projects"
        );
        assert_ne!(
            at_3.entry_key("articles", 7),
            at_3.list_key("articles", 20, Some(7))
        );
    }

    #[test]
    fn text_in_a_collection_name_cant_pose_as_another_field() {
        let at_3 = scope(3);
        let posing = format!("x:list:20:{:032x}:articles", 7);
        assert_ne!(
            at_3.list_key(&posing, 20, None),
            at_3.list_key("articles", 20, Some(7))
        );
        assert_ne!(
            at_3.entry_key("articles", 7),
            at_3.list_key(&format!("{:032x}:articles", 7), 20, None)
        );
    }

    #[test]
    fn tags_cover_the_project_environment_collection_and_entry() {
        let at_3 = scope(3);
        let (project, environment) = (format!("{:032x}", 1), format!("{:032x}", 14));
        assert_eq!(
            at_3.entry_tags("articles", 7),
            [
                format!("colophon:p:{project}"),
                format!("colophon:p:{project}:e:{environment}"),
                format!("colophon:p:{project}:e:{environment}:c:articles"),
                format!("colophon:p:{project}:e:{environment}:n:{:032x}", 7),
            ]
        );
        assert_eq!(at_3.list_tags("articles").len(), 3);
        // Tags don't depend on the generation: a purge finds every release's answers.
        assert_eq!(scope(4).list_tags("articles"), at_3.list_tags("articles"));
    }

    #[test]
    fn a_purge_names_each_entry_and_collection_once_and_stays_bounded() {
        let (project, environment) = (format!("{:032x}", 1), format!("{:032x}", 14));
        let env = format!("colophon:p:{project}:e:{environment}");
        let changed = [
            ("articles".to_owned(), 7),
            ("articles".to_owned(), 8),
            ("events".to_owned(), 9),
        ];
        assert_eq!(
            purge_tags(1, 14, &changed),
            [
                format!("{env}:c:articles"),
                format!("{env}:c:events"),
                format!("{env}:n:{:032x}", 7),
                format!("{env}:n:{:032x}", 8),
                format!("{env}:n:{:032x}", 9),
            ]
        );
        assert!(purge_tags(1, 14, &[]).is_empty(), "nothing changed");
        let many: Vec<(String, u128)> = (0..40)
            .map(|entry| ("articles".to_owned(), entry))
            .collect();
        assert_eq!(
            purge_tags(1, 14, &many),
            [env],
            "too many: the environment instead"
        );
        // The tags a purge names are the ones answers carry.
        let answer = scope(5).entry_tags("articles", 7);
        assert!(
            purge_tags(1, 14, &changed[..1])
                .iter()
                .all(|tag| answer.contains(tag))
        );
    }

    #[test]
    fn a_made_up_collection_name_gets_no_tag_of_its_own() {
        for name in ["", "Articles", "a,b", "a b", "ä", &"x".repeat(65)] {
            assert_eq!(scope(1).list_tags(name).len(), 2, "{name:?}");
        }
    }
}

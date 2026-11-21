//! What every target shares: the content type entries and queries are checked against, with every field
//! kind, the same one `crates/colophon-domain/tests/never_panics.rs` uses.

use colophon_domain::schema::ContentType;
use serde_json::json;

pub fn posts() -> ContentType {
    ContentType::parse(&json!({
        "name": "Posts",
        "title_field": "headline",
        "fields": [
            {"id": "headline", "label": "Headline", "type": "text", "required": true, "max_chars": 120, "index": true},
            {"id": "author", "label": "Author", "type": "reference", "to": "staff", "index": true},
            {"id": "publish_at", "label": "Publish at", "type": "datetime", "index": true},
            {"id": "section", "label": "Section", "type": "enum", "values": ["News", "Guides"], "index": true},
            {"id": "rating", "label": "Rating", "type": "number", "integer": true, "min": 0, "max": 5, "index": true},
            {"id": "score", "label": "Score", "type": "number", "index": true},
            {"id": "featured", "label": "Featured", "type": "boolean", "index": true},
            {"id": "slug", "label": "Slug", "type": "slug", "index": true},
            {"id": "email", "label": "Email", "type": "email", "index": true},
            {"id": "site", "label": "Site", "type": "url", "index": true},
            {"id": "born", "label": "Born", "type": "date", "index": true},
            {"id": "opens", "label": "Opens", "type": "time", "index": true},
            {"id": "notes", "label": "Notes", "type": "text", "multiline": true},
            {"id": "body", "label": "Body", "type": "rich_text"},
            {"id": "meta", "label": "Meta", "type": "object", "fields": [
                {"id": "seo", "label": "SEO", "type": "text"}
            ]},
            {"id": "tags", "label": "Tags", "type": "list", "of": {"type": "text"}},
            {"id": "related", "label": "Related", "type": "reference", "to": "articles", "many": true},
            {"id": "photo", "label": "Photo", "type": "asset"},
            {"id": "accent", "label": "Accent", "type": "color"},
            {"id": "scratch", "label": "Scratch", "type": "json"},
            {"id": "script", "label": "Script", "type": "markdown"}
        ]
    }))
    .expect("every kind parses")
}

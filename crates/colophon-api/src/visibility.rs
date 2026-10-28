//! What a role may see and change of one entry (D-058, step R.3): its fields, on its own entries
//! or anyone's. A read leaves out what the role can't read and says which fields it left out; a
//! save keeps those fields exactly as stored, and is refused, naming the field, if it changes one
//! the role can't change.

use colophon_domain::{
    policy::{Action, Fields, Policy},
    schema::Refusal,
};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::ApiError;

/// What a role may do with one entry's fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Access {
    /// The fields it may read, or `None` if it may not read the entry at all.
    pub(crate) read: Option<Fields>,
    /// The fields it may change, or `None` if it may change none.
    pub(crate) update: Option<Fields>,
}

impl Access {
    /// For `identity`, holding `policy`, on an entry of `collection` that `created_by` created.
    pub(crate) fn of(policy: &Policy, identity: Uuid, collection: &str, created_by: Uuid) -> Self {
        let own = identity == created_by;
        Self {
            read: policy.fields(Action::ContentRead, collection, own),
            update: policy.fields(Action::ContentUpdate, collection, own),
        }
    }
}

fn object(content: &str) -> Result<Map<String, Value>, ApiError> {
    match serde_json::from_str(content).map_err(ApiError::internal)? {
        Value::Object(fields) => Ok(fields),
        _ => Err(ApiError::internal(std::io::Error::other(
            "stored content is not a JSON object",
        ))),
    }
}

/// `content` with only the fields `read` allows, and which of the type's `fields` it hid.
pub(crate) fn project(
    content: String,
    read: &Fields,
    fields: &[String],
) -> Result<(String, Vec<String>), ApiError> {
    if *read == Fields::All {
        return Ok((content, Vec::new()));
    }
    let mut kept = object(&content)?;
    kept.retain(|field, _| read.includes(field));
    let hidden = fields
        .iter()
        .filter(|field| !read.includes(field))
        .cloned()
        .collect();
    Ok((
        serde_json::to_string(&kept).map_err(ApiError::internal)?,
        hidden,
    ))
}

/// The type's `fields` the role can read but not change: the editor shows them, locked.
pub(crate) fn locked(access: &Access, fields: &[String]) -> Vec<String> {
    fields
        .iter()
        .filter(|field| {
            access
                .read
                .as_ref()
                .is_some_and(|read| read.includes(field))
        })
        .filter(|field| {
            !access
                .update
                .as_ref()
                .is_some_and(|update| update.includes(field))
        })
        .cloned()
        .collect()
}

/// A save's `submitted` content with every field its author can't read put back as `stored`: they
/// never saw those fields, so leaving them out of a save mustn't erase them. Sending one with a
/// different value is refused, not dropped (D11): the role can't change what it can't read.
pub(crate) fn restore_hidden(
    submitted: Value,
    stored: &str,
    read: &Fields,
) -> Result<Value, ApiError> {
    if *read == Fields::All {
        return Ok(submitted);
    }
    let Value::Object(mut fields) = submitted else {
        // Not an object: validation refuses it, with its own words.
        return Ok(submitted);
    };
    let stored = object(stored)?;
    if let Some(field) = fields
        .iter()
        .find(|(field, value)| !read.includes(field) && stored.get(*field) != Some(*value))
        .map(|(field, _)| field.clone())
    {
        return Err(not_permitted(&field));
    }
    for (field, value) in stored {
        if !read.includes(&field) {
            fields.insert(field, value);
        }
    }
    Ok(Value::Object(fields))
}

/// Refuses a save that changes a field `update` doesn't allow, naming the first such field.
pub(crate) fn only_permitted_changes(
    saved: &str,
    stored: &str,
    update: &Fields,
) -> Result<(), ApiError> {
    if *update == Fields::All {
        return Ok(());
    }
    let (saved, stored) = (object(saved)?, object(stored)?);
    let mut changed: Vec<&String> = saved
        .keys()
        .chain(stored.keys())
        .filter(|field| saved.get(*field) != stored.get(*field))
        .collect();
    changed.sort();
    changed.dedup();
    match changed.into_iter().find(|field| !update.includes(field)) {
        None => Ok(()),
        Some(field) => Err(not_permitted(field)),
    }
}

/// Refuses content for a new entry that sets a field `create` doesn't allow.
pub(crate) fn only_permitted_fields(content: &str, create: &Fields) -> Result<(), ApiError> {
    if *create == Fields::All {
        return Ok(());
    }
    let fields = object(content)?;
    let mut set: Vec<&String> = fields
        .iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(field, _)| field)
        .collect();
    set.sort();
    match set.into_iter().find(|field| !create.includes(field)) {
        None => Ok(()),
        Some(field) => Err(not_permitted(field)),
    }
}

fn not_permitted(field: &str) -> ApiError {
    ApiError::Invalid(Refusal {
        code: "field_not_permitted",
        pointer: format!("/{field}"),
        message: format!("your role can't change the field {field}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only(fields: &[&str]) -> Fields {
        Fields::Only(fields.iter().map(|field| (*field).to_owned()).collect())
    }

    fn type_fields() -> Vec<String> {
        ["title", "body", "price"].map(str::to_owned).to_vec()
    }

    #[test]
    fn a_read_leaves_out_what_the_role_cannot_read_and_says_so() {
        let stored = r#"{"body":"Soup","price":12,"title":"Menu"}"#.to_owned();
        let (shown, hidden) =
            project(stored.clone(), &only(&["title", "body"]), &type_fields()).expect("projects");
        assert_eq!(shown, r#"{"body":"Soup","title":"Menu"}"#);
        assert_eq!(hidden, ["price"]);
        let (all, none) = project(stored.clone(), &Fields::All, &type_fields()).expect("projects");
        assert_eq!((all, none), (stored, Vec::<String>::new()));
    }

    #[test]
    fn a_save_keeps_hidden_fields_as_stored_and_refuses_changing_a_locked_one() {
        let stored = r#"{"body":"Soup","price":12,"title":"Menu"}"#;
        let read = only(&["title", "body"]);
        // The saver never saw the price: leaving it out keeps it; sending another is refused.
        let refused = restore_hidden(
            serde_json::json!({"title": "Spring menu", "body": "Soup", "price": 0}),
            stored,
            &read,
        );
        assert!(
            matches!(refused, Err(ApiError::Invalid(ref refusal)) if refusal.pointer == "/price")
        );
        let restored = restore_hidden(
            serde_json::json!({"title": "Spring menu", "body": "Soup"}),
            stored,
            &read,
        )
        .expect("restores");
        assert_eq!(restored["price"], 12);
        assert_eq!(restored["title"], "Spring menu");
        let saved = restored.to_string();
        assert!(only_permitted_changes(&saved, stored, &only(&["title"])).is_ok());
        let refused = only_permitted_changes(
            r#"{"body":"Stew","price":12,"title":"Menu"}"#,
            stored,
            &only(&["title"]),
        );
        assert!(
            matches!(refused, Err(ApiError::Invalid(ref refusal)) if refusal.code == "field_not_permitted" && refusal.pointer == "/body"),
            "{refused:?}"
        );
        // Removing a field is a change too.
        assert!(
            only_permitted_changes(r#"{"price":12,"title":"Menu"}"#, stored, &only(&["title"]))
                .is_err()
        );
    }

    #[test]
    fn a_new_entry_may_only_set_the_fields_the_role_may_create() {
        assert!(
            only_permitted_fields(r#"{"title":"Menu","body":null}"#, &only(&["title"])).is_ok()
        );
        assert!(only_permitted_fields(r#"{"title":"Menu","price":3}"#, &only(&["title"])).is_err());
        assert!(only_permitted_fields(r#"{"price":3}"#, &Fields::All).is_ok());
    }

    #[test]
    fn locked_fields_are_readable_but_not_changeable() {
        let access = Access {
            read: Some(only(&["title", "body"])),
            update: Some(only(&["title"])),
        };
        assert_eq!(locked(&access, &type_fields()), ["body"]);
    }
}

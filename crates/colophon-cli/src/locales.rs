//! `colophon locales list` and `colophon locales set` (docs/modules/08-cli.md): the
//! environment's languages — each one's label, tag, default flag and fallback chain, in
//! display order — read, and replaced whole from a file.
//!
//! `set` reads the current list's `ETag` first and saves on top of exactly that version
//! (`If-Match`), so a change someone made in between is refused, never overwritten; the write
//! carries one `Idempotency-Key`, reused on a lost answer (`scope::send_write`). A refusal is
//! told in plain words — which language in the file, and what to do — while `--json` keeps
//! the API's error object whole.

use reqwest::Method;
use serde_json::{Value, json};

use crate::{
    Exit,
    client::{Failure, Reply},
    out::Out,
    scope::{self, Scope},
};

/// `{P}/locales`.
fn locales_path(project: &str, scope: &Scope<'_>) -> String {
    format!("{}/locales", scope::env_path(project, scope))
}

/// The `ETag` a reply carried (`"locales-3"`), or empty when it sent none.
fn etag_of(reply: &Reply) -> String {
    reply
        .headers
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

/// Runs `colophon locales list`.
pub(crate) async fn list(scope: &Scope<'_>, out: Out) -> Result<(), Failure> {
    let (client, project) = scope::open(scope).await?;
    let reply = client
        .send(Method::GET, &locales_path(&project, scope), None, &[])
        .await?;
    let data = &reply.body["data"];
    if out.format.machine() {
        out.print_data(&json!({
            "version": data["version"],
            "etag": etag_of(&reply),
            "locales": data["locales"],
        }));
    } else {
        for line in render(data) {
            println!("{line}");
        }
    }
    Ok(())
}

/// Runs `colophon locales set`: `document` is the file's `{"locales": [...]}`.
pub(crate) async fn set(
    scope: &Scope<'_>,
    document: &Value,
    key: Option<&str>,
    out: Out,
) -> Result<(), Failure> {
    let body = request_body(document)?;
    let (client, project) = scope::open(scope).await?;
    let path = locales_path(&project, scope);
    let current = client.send(Method::GET, &path, None, &[]).await?;
    let etag = etag_of(&current);
    if etag.is_empty() {
        return Err(Failure::new(
            Exit::Internal,
            "the server answered the languages without an ETag to save on top of",
        ));
    }
    if current.body["data"]["locales"] == body["locales"] {
        // Nothing to write: saving the same list would only move the version on.
        let data = &current.body["data"];
        if out.format.machine() {
            out.print_data(&json!({
                "status": "unchanged",
                "version": data["version"],
                "etag": etag,
                "locales": data["locales"],
            }));
        } else {
            println!(
                "The languages already match the file (version {}); nothing to save.",
                data["version"].as_i64().unwrap_or_default()
            );
        }
        return Ok(());
    }
    let key = key.map_or_else(scope::new_key, str::to_owned);
    let reply = scope::send_write(
        &client,
        Method::PUT,
        &path,
        Some(&body),
        &[("idempotency-key", &key), ("if-match", &etag)],
    )
    .await
    .map_err(|failure| explain(failure, &body))?;
    let replayed = reply
        .headers
        .get("idempotent-replayed")
        .is_some_and(|value| value == "true");
    let data = &reply.body["data"];
    if out.format.machine() {
        out.print_data(&json!({
            "status": if replayed { "replayed" } else { "updated" },
            "idempotency_key": key,
            "version": data["version"],
            "etag": etag_of(&reply),
            "locales": data["locales"],
        }));
    } else {
        println!(
            "{} (version {}).",
            if replayed {
                "Already saved"
            } else {
                "Saved the languages"
            },
            data["version"].as_i64().unwrap_or_default()
        );
        for line in render(data).iter().skip(1) {
            println!("{line}");
        }
    }
    Ok(())
}

/// The `PUT` body a file's document makes: exactly `{"locales": [...]}`, each language an
/// object. Its contents — tags, labels, the one default, fallbacks — are the API's to judge,
/// so a refusal names the same rule on every surface.
fn request_body(document: &Value) -> Result<Value, Failure> {
    const SHAPE: &str = "the file must be {\"locales\": [...]}: the whole list, in order, each \
                         language {\"tag\", \"label\", \"default\", \"fallback\"}";
    let object = document
        .as_object()
        .ok_or_else(|| Failure::new(Exit::Usage, SHAPE))?;
    if let Some(other) = object.keys().find(|key| *key != "locales") {
        return Err(Failure::new(
            Exit::Usage,
            format!("the file holds only \"locales\", not {other:?}"),
        ));
    }
    let locales = object
        .get("locales")
        .and_then(Value::as_array)
        .ok_or_else(|| Failure::new(Exit::Usage, SHAPE))?;
    if let Some(index) = locales.iter().position(|locale| !locale.is_object()) {
        return Err(Failure::new(
            Exit::Usage,
            format!(
                "language {} in the file is not an object: {SHAPE}",
                index + 1
            ),
        ));
    }
    Ok(json!({ "locales": locales }))
}

/// The list as a person reads it: one line a language — label, tag, and whether it is the
/// default or where it falls back — under a header naming the version.
fn render(data: &Value) -> Vec<String> {
    let version = data["version"].as_i64().unwrap_or_default();
    let locales = data["locales"].as_array().map_or(&[][..], Vec::as_slice);
    if locales.is_empty() {
        return vec![format!(
            "No languages listed (version {version}): entries may carry any locale tag."
        )];
    }
    let text = |locale: &Value, field: &str| locale[field].as_str().unwrap_or("?").to_owned();
    let label_width = locales
        .iter()
        .map(|locale| text(locale, "label").chars().count())
        .max()
        .unwrap_or_default();
    let tag_width = locales
        .iter()
        .map(|locale| text(locale, "tag").chars().count())
        .max()
        .unwrap_or_default();
    let mut lines = vec![format!("Languages (version {version}):")];
    for locale in locales {
        let fallback: Vec<&str> = locale["fallback"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let falls = if fallback.is_empty() {
            "no fallback".to_owned()
        } else {
            format!("falls back to {}", fallback.join(", "))
        };
        let role = if locale["default"].as_bool() == Some(true) {
            format!("default, {falls}")
        } else {
            falls
        };
        lines.push(format!(
            "  {:<label_width$}  {:<tag_width$}  {role}",
            text(locale, "label"),
            text(locale, "tag"),
        ));
    }
    lines
}

/// The position `/locales/{i}/…` names, as the file's 1-based language number and its tag.
fn language_at(pointer: &str, body: &Value) -> Option<(usize, String)> {
    let index: usize = pointer
        .strip_prefix("/locales/")?
        .split('/')
        .next()?
        .parse()
        .ok()?;
    let tag = body["locales"][index]["tag"]
        .as_str()
        .map_or_else(|| "no tag".to_owned(), |tag| format!("{tag:?}"));
    Some((index + 1, tag))
}

/// A refused save, told in plain words: which language in the file and what to do. The
/// exit code and the API's error object (`--json`) are kept exactly as they came.
fn explain(failure: Failure, body: &Value) -> Failure {
    let Some(refusal) = failure.refusal.as_ref() else {
        return failure;
    };
    let details = &refusal["details"];
    let reason = details["reason"].as_str().unwrap_or_default();
    let pointer = details["pointer"].as_str().unwrap_or_default();
    let api_message = refusal["message"].as_str().unwrap_or_default();
    let at = language_at(pointer, body)
        .map(|(number, tag)| format!("language {number} ({tag}) in the file: "))
        .unwrap_or_default();
    let count = body["locales"].as_array().map_or(0, Vec::len);
    let plain = match reason {
        "stale_locales" => "someone changed the languages after this command read them, so \
                            nothing was saved; check `colophon locales list`, then run it again"
            .to_owned(),
        "locale_in_use" => {
            let total = details["total"].as_i64().unwrap_or_default();
            let named: Vec<String> = details["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .take(5)
                .map(|entry| {
                    format!(
                        "{:?} ({})",
                        entry["title"].as_str().unwrap_or(""),
                        entry["entry"].as_str().unwrap_or("?")
                    )
                })
                .collect();
            let more = usize::try_from(total)
                .unwrap_or_default()
                .saturating_sub(named.len());
            let more = if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            };
            format!(
                "{api_message}, so that language can't be removed: {}{more}. Move or delete \
                 those entries first, or keep the language in the file",
                named.join(", ")
            )
        }
        "missing_permission" => {
            "changing the languages needs the schema.change permission, which your role here \
             doesn't have"
                .to_owned()
        }
        "invalid_locale" => format!(
            "{at}not a language tag: 2–3 letters, then optional parts after '-' of 1–8 letters \
             or digits, such as en, pt-BR or zh-Hant"
        ),
        "invalid_locale_label" => format!("{at}the label must be 1–80 characters"),
        "duplicate_locale" => format!("{at}that tag is already listed (case doesn't count)"),
        "too_many_locales" => {
            format!("an environment lists at most 50 languages; the file has {count}")
        }
        "locale_default_required" => {
            "exactly one language must be the default (\"default\": true)".to_owned()
        }
        "fallback_not_listed" => {
            let fallback = pointer
                .rsplit('/')
                .next()
                .and_then(|index| index.parse::<usize>().ok())
                .and_then(|index| language_at(pointer, body).map(|(number, _)| (number, index)))
                .and_then(|(number, index)| body["locales"][number - 1]["fallback"][index].as_str())
                .map_or_else(String::new, |tag| format!(" {tag:?}"));
            format!("{at}it falls back to{fallback}, which isn't in the list; add it or drop it")
        }
        "invalid_fallback" => format!(
            "{at}a fallback chain can't name the language itself or a tag twice, and holds at \
             most 4"
        ),
        _ => return failure,
    };
    let status = failure
        .message
        .rfind(" [HTTP ")
        .map_or("", |at| &failure.message[at..]);
    Failure {
        message: format!("{plain}{status}"),
        ..failure
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(exit: Exit, status: u16, error: Value) -> Failure {
        Failure {
            exit,
            message: format!(
                "{} [HTTP {status}]",
                error["message"].as_str().unwrap_or("")
            ),
            refusal: Some(error),
        }
    }

    fn file() -> Value {
        json!({"locales": [
            {"tag": "en", "label": "English", "default": true, "fallback": []},
            {"tag": "pt", "label": "Português", "default": false, "fallback": ["en"]},
            {"tag": "pt-BR", "label": "Português (Brasil)", "default": false, "fallback": ["pt", "xx"]},
        ]})
    }

    #[test]
    fn the_file_is_exactly_a_list_of_language_objects() {
        assert_eq!(request_body(&file()).expect("a list"), file());
        assert_eq!(
            request_body(&json!({"locales": []})).expect("an empty list is one free language"),
            json!({"locales": []})
        );
        for (document, says) in [
            (json!([{"tag": "en"}]), "must be {\"locales\""),
            (json!({"languages": []}), "not \"languages\""),
            (json!({"locales": [], "version": 3}), "not \"version\""),
            (json!({"locales": {"tag": "en"}}), "must be {\"locales\""),
            (json!({"locales": [{"tag": "en"}, "fr"]}), "language 2"),
        ] {
            let failure = request_body(&document).expect_err("refused");
            assert_eq!(failure.exit, Exit::Usage, "{document}");
            assert!(failure.message.contains(says), "{}", failure.message);
        }
    }

    #[test]
    fn the_list_reads_label_tag_and_role_in_aligned_columns() {
        let data = json!({"version": 3, "locales": file()["locales"]});
        assert_eq!(
            render(&data),
            [
                "Languages (version 3):",
                "  English             en     default, no fallback",
                "  Português           pt     falls back to en",
                "  Português (Brasil)  pt-BR  falls back to pt, xx",
            ]
        );
        assert_eq!(
            render(&json!({"version": 0, "locales": []})),
            ["No languages listed (version 0): entries may carry any locale tag."]
        );
    }

    #[test]
    fn a_pointer_names_the_language_in_the_file() {
        let body = file();
        assert_eq!(
            language_at("/locales/2/fallback/1", &body),
            Some((3, "\"pt-BR\"".to_owned()))
        );
        assert_eq!(
            language_at("/locales/0/tag", &body),
            Some((1, "\"en\"".to_owned()))
        );
        assert_eq!(language_at("/locales", &body), None);
        assert_eq!(language_at("/locale", &body), None);
        assert_eq!(
            language_at("/locales/9/tag", &body),
            Some((10, "no tag".to_owned()))
        );
    }

    #[test]
    fn a_refusal_is_told_in_plain_words_with_its_exit_and_envelope_kept() {
        let body = file();
        let cases = [
            (
                Exit::Conflict,
                409,
                json!({"reason": "stale_locales"}),
                "someone changed the languages",
            ),
            (
                Exit::Usage,
                422,
                json!({"reason": "invalid_locale", "pointer": "/locales/1/tag"}),
                "language 2 (\"pt\") in the file: not a language tag",
            ),
            (
                Exit::Usage,
                422,
                json!({"reason": "duplicate_locale", "pointer": "/locales/0/tag"}),
                "language 1 (\"en\") in the file: that tag is already listed",
            ),
            (
                Exit::Usage,
                422,
                json!({"reason": "fallback_not_listed", "pointer": "/locales/2/fallback/1"}),
                "language 3 (\"pt-BR\") in the file: it falls back to \"xx\", which isn't in the list",
            ),
            (
                Exit::Usage,
                422,
                json!({"reason": "invalid_fallback", "pointer": "/locales/2/fallback"}),
                "language 3 (\"pt-BR\") in the file: a fallback chain",
            ),
            (
                Exit::Usage,
                422,
                json!({"reason": "locale_default_required", "pointer": "/locales"}),
                "exactly one language must be the default",
            ),
            (
                Exit::Usage,
                422,
                json!({"reason": "too_many_locales", "pointer": "/locales"}),
                "at most 50 languages; the file has 3",
            ),
            (
                Exit::Usage,
                422,
                json!({"reason": "invalid_locale_label", "pointer": "/locales/0/label"}),
                "language 1 (\"en\") in the file: the label must be 1–80 characters",
            ),
            (
                Exit::Denied,
                403,
                json!({"reason": "missing_permission", "permission": "schema.change"}),
                "needs the schema.change permission",
            ),
        ];
        for (exit, status, details, says) in cases {
            let error = json!({
                "code": "refused", "message": "the API's words", "details": details, "request_id": "r1"
            });
            let told = explain(refused(exit, status, error.clone()), &body);
            assert!(told.message.contains(says), "{}", told.message);
            assert!(
                told.message.ends_with(&format!("[HTTP {status}]")),
                "{}",
                told.message
            );
            assert_eq!(told.exit, exit, "the exit class is the API's");
            assert_eq!(
                told.refusal,
                Some(error),
                "--json keeps the API's object whole"
            );
        }
    }

    #[test]
    fn a_language_in_use_names_the_entries_in_the_way() {
        let error = json!({
            "code": "conflict", "message": "12 entries are written in pt-BR",
            "details": {
                "reason": "locale_in_use", "pointer": "/locales", "total": 12,
                "entries": [
                    {"entry": "e1", "title": "Olá", "state": "draft"},
                    {"entry": "e2", "title": "Tchau", "state": "draft"},
                ]
            },
            "request_id": "r1"
        });
        let told = explain(refused(Exit::Conflict, 409, error), &file());
        assert_eq!(
            told.message,
            "12 entries are written in pt-BR, so that language can't be removed: \"Olá\" (e1), \
             \"Tchau\" (e2) and 10 more. Move or delete those entries first, or keep the \
             language in the file [HTTP 409]"
        );
        assert_eq!(told.exit, Exit::Conflict);
    }

    #[test]
    fn an_unknown_refusal_or_a_lost_answer_stays_as_it_came() {
        let error = json!({"code": "validation_failed", "message": "something new", "details": {"reason": "brand_new"}});
        let told = explain(refused(Exit::Usage, 422, error), &file());
        assert_eq!(told.message, "something new [HTTP 422]");
        let lost = explain(Failure::new(Exit::Unavailable, "no answer"), &file());
        assert_eq!(lost.message, "no answer");
        assert_eq!(lost.exit, Exit::Unavailable);
    }
}

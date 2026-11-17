//! Languages end to end (D-177, D-178): the real binary, a real sign-in. `GET|PUT {P}/locales`
//! replaces an environment's language list under its version and guards what it drops; entry
//! writes keep to the list; `/content/v1` answers in one language with `?locale=` and
//! `?fallback=` — one row per translation group, labelled, paged by group, keyed apart in the
//! cache — and with every language, as before, without them. Needs the stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{ORIGIN, Reply, Server, Session, app, call, join, key, project, release, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

const PAGES: &str = "pages";

fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
}

fn entries(project: &str) -> String {
    format!("{}/collections/{PAGES}/entries", base(project))
}

fn delivery(project: &str) -> String {
    format!("/content/v1/{project}/production/{PAGES}")
}

async fn send(session: &Session, method: Method, path: &str, body: Option<&Value>) -> Reply {
    call(session, method, path, body, &[("idempotency-key", &key())])
        .await
        .expect("request")
}

/// `PUT {P}/locales` with `list`, naming version `version` in `If-Match`.
async fn put_locales(session: &Session, project: &str, version: i64, list: &Value) -> Reply {
    call(
        session,
        Method::PUT,
        &format!("{}/locales", base(project)),
        Some(&json!({ "locales": list })),
        &[
            ("if-match", &format!("\"locales-{version}\"")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request")
}

/// An anonymous delivery read.
async fn read(path: &str) -> Reply {
    let response = app()
        .get(format!("{ORIGIN}{path}"))
        .send()
        .await
        .expect("request");
    Reply {
        status: response.status(),
        headers: response.headers().clone(),
        body: response.json().await.unwrap_or(Value::Null),
    }
}

/// An anonymous delivery query.
async fn query(path: &str, document: &Value) -> Reply {
    let response = app()
        .post(format!("{ORIGIN}{path}"))
        .json(document)
        .send()
        .await
        .expect("request");
    Reply {
        status: response.status(),
        headers: response.headers().clone(),
        body: response.json().await.unwrap_or(Value::Null),
    }
}

/// Reads `path` until the cache serves it: a busy machine can make Valkey miss its deadline
/// (`bypass`, and the cache is left alone for a second), which says nothing about the keys.
async fn cached(path: &str) -> Reply {
    for _ in 0..10 {
        let reply = read(path).await;
        if header(&reply, "colophon-cache") == Some("hit") {
            return reply;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    }
    panic!("{path} was never served from the cache");
}

fn header<'a>(reply: &'a Reply, name: &str) -> Option<&'a str> {
    reply
        .headers
        .get(name)
        .and_then(|value| value.to_str().ok())
}

fn reason(reply: &Reply) -> &str {
    reply.body["error"]["details"]["reason"]
        .as_str()
        .unwrap_or_default()
}

/// `(title, locale, fallback)` of a page's rows, sorted by title.
fn rows(reply: &Reply) -> Vec<(String, String, Option<bool>)> {
    let mut rows: Vec<_> = reply.body["data"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|row| {
            (
                row["title"].as_str().expect("title").to_owned(),
                row["locale"].as_str().unwrap_or("-").to_owned(),
                row["localization"]["fallback"].as_bool(),
            )
        })
        .collect();
    rows.sort();
    rows
}

fn row(title: &str, locale: &str, fallback: Option<bool>) -> (String, String, Option<bool>) {
    (title.to_owned(), locale.to_owned(), fallback)
}

/// A project with the `pages` type (an indexed title, so a query can filter on it).
async fn set_up() -> (Server, Session, String) {
    let server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let created = send(
        &owner,
        Method::POST,
        &format!("{}/content-types", base(&project)),
        Some(&json!({"slug": PAGES, "definition": {
        "name": "Pages", "title_field": "title", "fields": [
            {"id": "title", "label": "Title", "type": "text", "required": true, "index": true},
            {"id": "slug", "label": "URL name", "type": "slug"}
        ]}})),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    (server, owner, project)
}

/// en (default), pt → en, pt-BR → pt, en, fr → en.
fn languages() -> Value {
    json!([
        {"tag": "en", "label": "English", "default": true},
        {"tag": "pt", "label": "Português", "fallback": ["en"]},
        {"tag": "pt-BR", "label": "Português (Brasil)", "fallback": ["pt", "en"]},
        {"tag": "fr", "label": "Français", "fallback": ["en"]}
    ])
}

/// Creates a page titled `title` in `locale`; its id.
async fn page(
    owner: &Session,
    project: &str,
    title: &str,
    slug: &str,
    locale: Option<&str>,
) -> String {
    let path = match locale {
        Some(locale) => format!("{}?locale={locale}", entries(project)),
        None => entries(project),
    };
    let created = send(
        owner,
        Method::POST,
        &path,
        Some(&json!({"title": title, "slug": slug})),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    created.body["data"]["id"].as_str().expect("id").to_owned()
}

/// Adds `source`'s translation in `locale` and retitles it `title`; its id.
async fn translate(
    owner: &Session,
    project: &str,
    source: &str,
    locale: &str,
    title: &str,
) -> String {
    let made = send(
        owner,
        Method::POST,
        &format!(
            "{}/{source}/localizations?locale={locale}",
            entries(project)
        ),
        None,
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let id = made.body["data"]["id"].as_str().expect("id").to_owned();
    let revision = made.body["data"]["revision"]["id"]
        .as_str()
        .expect("revision");
    let slug = made.body["data"]["content"]["slug"].clone();
    let saved = call(
        owner,
        Method::PUT,
        &format!("{}/{id}", entries(project)),
        Some(&json!({"title": title, "slug": slug})),
        &[
            ("if-match", &format!("\"{revision}\"")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request");
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    id
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: read, refuse, replace, replay, conflict, guard"
)]
async fn languages_are_listed_replaced_and_guarded() {
    let (_server, owner, project) = set_up().await;
    let path = format!("{}/locales", base(&project));

    // Before any change: one language, free tags, version 0.
    let empty = send(&owner, Method::GET, &path, None).await;
    assert_eq!(empty.status, StatusCode::OK);
    assert_eq!(empty.body["data"], json!({"version": 0, "locales": []}));
    assert_eq!(empty.etag().as_deref(), Some("\"locales-0\""));
    let free = page(&owner, &project, "Free", "free", Some("de")).await;
    // Without a list a reader may ask for any tag, but not name fallbacks: they would give
    // every made-up tag an answer worth caching.
    let unlisted_read = read(&format!("{}?locale=zz&fallback=de", delivery(&project))).await;
    assert_eq!(unlisted_read.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reason(&unlisted_read), "invalid_fallback");
    let made_up = read(&format!("{}?locale=zz", delivery(&project))).await;
    assert_eq!(made_up.status, StatusCode::OK);
    assert_eq!(made_up.body["data"], json!([]));

    // A replace needs If-Match, and a list that holds together.
    let unmatched = send(&owner, Method::PUT, &path, Some(&json!({"locales": []}))).await;
    assert_eq!(unmatched.status, StatusCode::PRECONDITION_REQUIRED);
    let unlisted = put_locales(
        &owner,
        &project,
        0,
        &json!([{"tag": "en", "label": "English", "default": true, "fallback": ["fr"]}]),
    )
    .await;
    assert_eq!(unlisted.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reason(&unlisted), "fallback_not_listed");
    assert_eq!(
        unlisted.body["error"]["details"]["pointer"],
        "/locales/0/fallback/0"
    );
    let no_default = put_locales(
        &owner,
        &project,
        0,
        &json!([{"tag": "en", "label": "English"}]),
    )
    .await;
    assert_eq!(reason(&no_default), "locale_default_required");

    // The German page is in use: a list without `de` is refused, naming it.
    let in_use = put_locales(&owner, &project, 0, &languages()).await;
    assert_eq!(in_use.status, StatusCode::CONFLICT, "{}", in_use.body);
    assert_eq!(reason(&in_use), "locale_in_use");
    assert_eq!(in_use.body["error"]["details"]["total"], 1);
    assert_eq!(in_use.body["error"]["details"]["locale"], "de");
    assert_eq!(in_use.body["error"]["details"]["entries"][0]["entry"], free);

    // With `de` listed too, it goes through: version 1, and a replay answers the same.
    let mut list = languages();
    list.as_array_mut()
        .expect("a list")
        .push(json!({"tag": "de", "label": "Deutsch", "fallback": ["en"]}));
    let replace_key = key();
    let replace = |key: String| {
        let (owner, path, list) = (&owner, &path, &list);
        async move {
            call(
                owner,
                Method::PUT,
                path,
                Some(&json!({ "locales": list })),
                &[("if-match", "\"locales-0\""), ("idempotency-key", &key)],
            )
            .await
            .expect("request")
        }
    };
    let replaced = replace(replace_key.clone()).await;
    assert_eq!(replaced.status, StatusCode::OK, "{}", replaced.body);
    assert_eq!(replaced.etag().as_deref(), Some("\"locales-1\""));
    assert_eq!(replaced.body["data"]["version"], 1);
    assert_eq!(
        replaced.body["data"]["locales"][2]["fallback"],
        json!(["pt", "en"])
    );
    let replay = replace(replace_key).await;
    assert!(replay.replayed());
    assert_eq!(replay.body, replaced.body);

    // A replace that saw version 0 is stale now, and says what the list's ETag is.
    let stale = replace(key()).await;
    assert_eq!(stale.status, StatusCode::CONFLICT);
    assert_eq!(reason(&stale), "stale_locales");
    assert_eq!(stale.etag().as_deref(), Some("\"locales-1\""));

    // Entry writes keep to the list: an unlisted tag is refused, a listed one is stored in the
    // list's spelling.
    let refused = send(
        &owner,
        Method::POST,
        &format!("{}?locale=it", entries(&project)),
        Some(&json!({"title": "Ciao", "slug": "ciao"})),
    )
    .await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reason(&refused), "locale_not_configured");
    assert_eq!(refused.body["error"]["details"]["pointer"], "/locale");
    let spelled = send(
        &owner,
        Method::POST,
        &format!("{}?locale=PT-br", entries(&project)),
        Some(&json!({"title": "Olá", "slug": "ola"})),
    )
    .await;
    assert_eq!(spelled.body["data"]["locale"], "pt-BR");
    let localize = send(
        &owner,
        Method::POST,
        &format!("{}/{free}/localizations?locale=it", entries(&project)),
        None,
    )
    .await;
    assert_eq!(reason(&localize), "locale_not_configured");
    // A translation into a tag with capitals derives a slug a slug field accepts: lowercase
    // (it was refused `invalid_slug`, so no slugged page could get a pt-BR version).
    let brazilian = send(
        &owner,
        Method::POST,
        &format!("{}/{free}/localizations?locale=pt-BR", entries(&project)),
        None,
    )
    .await;
    assert_eq!(brazilian.status, StatusCode::CREATED, "{}", brazilian.body);
    assert_eq!(brazilian.body["data"]["content"]["slug"], "free-pt-br");
    assert_eq!(brazilian.body["data"]["locale"], "pt-BR");

    // Only a role with schema.change may replace it; anyone in the project may read it.
    let editor = sign_in("dev-editor").await;
    join(&owner, &editor, &project, "editor").await;
    assert_eq!(
        send(&editor, Method::GET, &path, None).await.status,
        StatusCode::OK
    );
    let forbidden = put_locales(&editor, &project, 1, &languages()).await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: every language, one language, fallbacks, pages, entries, unpublish, policy"
)]
async fn delivery_answers_in_one_language_through_its_fallbacks() {
    let (_server, owner, project) = set_up().await;
    let set = put_locales(&owner, &project, 0, &languages()).await;
    assert_eq!(set.status, StatusCode::OK, "{}", set.body);

    // Home in en, pt and pt-BR; News in en and fr; About in en; Plain untagged.
    let home = page(&owner, &project, "Home", "home", Some("en")).await;
    let home_pt = translate(&owner, &project, &home, "pt", "Início").await;
    let home_br = translate(&owner, &project, &home, "pt-BR", "Início (BR)").await;
    let news = page(&owner, &project, "News", "news", Some("en")).await;
    let news_fr = translate(&owner, &project, &news, "fr", "Nouvelles").await;
    let about = page(&owner, &project, "About", "about", Some("en")).await;
    let plain = page(&owner, &project, "Plain", "plain", None).await;
    let first = release(
        &owner,
        &project,
        &json!({"title": "Everything", "entries": [home, home_pt, home_br, news, news_fr, about, plain]}),
    )
    .await;
    let list = delivery(&project);

    // Without ?locale every language comes back, each row naming its own; an untagged entry
    // reads as the default language.
    let every = read(&list).await;
    assert_eq!(every.status, StatusCode::OK, "{}", every.body);
    assert_eq!(header(&every, "colophon-locale-policy"), Some("1"));
    assert_eq!(header(&every, "content-language"), None);
    assert_eq!(
        rows(&every),
        vec![
            row("About", "en", None),
            row("Home", "en", None),
            row("Início", "pt", None),
            row("Início (BR)", "pt-BR", None),
            row("News", "en", None),
            row("Nouvelles", "fr", None),
            row("Plain", "en", None),
        ]
    );

    // pt-BR through its policy (pt, then en): one row per document, labelled.
    let brazil = read(&format!("{list}?locale=pt-br")).await;
    assert_eq!(brazil.status, StatusCode::OK, "{}", brazil.body);
    assert_eq!(header(&brazil, "content-language"), Some("pt-BR"));
    assert_eq!(
        rows(&brazil),
        vec![
            row("About", "en", Some(true)),
            row("Início (BR)", "pt-BR", Some(false)),
            row("News", "en", Some(true)),
            row("Plain", "en", Some(true)),
        ]
    );
    assert_eq!(brazil.body["data"][0]["localization"]["requested"], "pt-BR");
    // No fallback: exactly pt-BR. An explicit chain: fr, then pt.
    assert_eq!(
        rows(&read(&format!("{list}?locale=pt-BR&fallback=none")).await),
        vec![row("Início (BR)", "pt-BR", Some(false))]
    );
    assert_eq!(
        rows(&read(&format!("{list}?locale=fr&fallback=pt")).await),
        vec![
            row("Início", "pt", Some(true)),
            row("Nouvelles", "fr", Some(false))
        ]
    );
    // What the list doesn't have, or a tag that isn't one, is refused.
    for (path, code) in [
        (format!("{list}?locale=de"), "unknown_locale"),
        (format!("{list}?locale=pt-BR&fallback=de"), "unknown_locale"),
        (format!("{list}?locale=en_GB"), "invalid_locale"),
        (format!("{list}?fallback=none"), "invalid_fallback"),
        (
            format!("{list}?locale=pt&fallback=a,b,c,d,e"),
            "invalid_fallback",
        ),
    ] {
        let refused = read(&path).await;
        assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY, "{path}");
        assert_eq!(reason(&refused), code, "{path}");
    }

    // The cache keeps languages apart: a later read is a hit, another language never one —
    // its first read has a key of its own — with its own body.
    let again = cached(&format!("{list}?locale=pt-BR")).await;
    assert_eq!(again.body, brazil.body);
    let portuguese = read(&format!("{list}?locale=pt")).await;
    assert_ne!(header(&portuguese, "colophon-cache"), Some("hit"));
    assert_ne!(portuguese.body, brazil.body);

    // Pages walk groups: two of four, then the rest; a cursor is its language's alone.
    let one = read(&format!("{list}?locale=en&limit=2")).await;
    let cursor = one.body["next_cursor"]
        .as_str()
        .expect("a next page")
        .to_owned();
    let two = read(&format!("{list}?locale=en&limit=2&cursor={cursor}")).await;
    assert_eq!(two.status, StatusCode::OK, "{}", two.body);
    assert_eq!(two.body["next_cursor"], Value::Null);
    let mut paged: Vec<_> = rows(&one).into_iter().chain(rows(&two)).collect();
    paged.sort();
    assert_eq!(
        paged,
        vec![
            row("About", "en", Some(false)),
            row("Home", "en", Some(false)),
            row("News", "en", Some(false)),
            row("Plain", "en", Some(false)),
        ]
    );
    let foreign = read(&format!("{list}?locale=fr&limit=2&cursor={cursor}")).await;
    assert_eq!(foreign.status, StatusCode::BAD_REQUEST);

    // An entry read answers the best translation, says which was asked for, and lists the
    // others for hreflang.
    let entry = read(&format!("{list}/{home}?locale=pt-BR")).await;
    assert_eq!(entry.status, StatusCode::OK, "{}", entry.body);
    assert_eq!(entry.body["data"]["id"], home_br.as_str());
    assert_eq!(entry.body["data"]["requested_id"], home.as_str());
    assert_eq!(entry.body["data"]["localization"]["fallback"], false);
    let mut alternates: Vec<_> = entry.body["data"]["alternates"]
        .as_array()
        .expect("alternates")
        .iter()
        .map(|alternate| alternate["locale"].as_str().expect("a tag").to_owned())
        .collect();
    alternates.sort();
    assert_eq!(alternates, ["en", "pt"]);
    let plain_read = read(&format!("{list}/{home}")).await;
    assert_eq!(plain_read.body["data"]["id"], home.as_str());
    assert_eq!(plain_read.body["data"]["locale"], "en");
    assert_eq!(
        plain_read.body["data"]["alternates"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert!(plain_read.body["data"].get("requested_id").is_none());
    let missing = read(&format!("{list}/{about}?locale=fr&fallback=none")).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(reason(&missing), "not_in_locale");
    assert_eq!(missing.body["error"]["details"]["available"], json!(["en"]));

    // A query resolves first, then filters: "Início (BR)" is what pt-BR readers get, and
    // nothing English readers get is titled that.
    let filter = json!({"filters": [{"field": "title", "op": "eq", "value": "Início (BR)"}]});
    let queried = query(&format!("{list}/query?locale=pt-BR"), &filter).await;
    assert_eq!(queried.status, StatusCode::OK, "{}", queried.body);
    assert_eq!(
        rows(&queried),
        vec![row("Início (BR)", "pt-BR", Some(false))]
    );
    let english = query(&format!("{list}/query?locale=en"), &filter).await;
    assert_eq!(english.body["data"], json!([]));
    let all = query(&format!("{list}/query?locale=pt-BR"), &json!({})).await;
    assert_eq!(all.body["data"].as_array().map(Vec::len), Some(4));

    // Unpublishing the Brazilian page leaves the others: pt-BR readers fall back to pt now —
    // at the release before too, since a takedown reaches every generation (DL5).
    let second = release(
        &owner,
        &project,
        &json!({"title": "Take pt-BR down", "unpublish": [home_br]}),
    )
    .await;
    assert!(second > first);
    assert_eq!(
        rows(&read(&format!("{list}?locale=pt-BR")).await)[1],
        row("Início", "pt", Some(true))
    );
    assert_eq!(
        rows(&read(&format!("{list}?locale=pt-BR&generation={first}")).await)[1],
        row("Início", "pt", Some(true))
    );
    assert_eq!(
        read(&format!("{list}/{home_br}?locale=pt-BR&fallback=none"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // A policy change is a new key: pt-BR now falls back to en alone — for pinned reads too,
    // which resolve with the current policy unless they name their fallbacks.
    let mut changed = languages();
    changed[2]["fallback"] = json!(["en"]);
    let moved = put_locales(&owner, &project, 1, &changed).await;
    assert_eq!(moved.status, StatusCode::OK, "{}", moved.body);
    let after = read(&format!("{list}?locale=pt-BR")).await;
    assert_eq!(header(&after, "colophon-locale-policy"), Some("2"));
    assert_ne!(header(&after, "colophon-cache"), Some("hit"));
    assert_eq!(rows(&after)[1], row("Home", "en", Some(true)));
    let pinned = read(&format!("{list}?locale=pt-BR&generation={first}")).await;
    assert_eq!(rows(&pinned)[1], row("Home", "en", Some(true)));
    let named = read(&format!(
        "{list}?locale=pt-BR&fallback=pt&generation={first}"
    ))
    .await;
    assert_eq!(rows(&named), vec![row("Início", "pt", Some(true))]);
}

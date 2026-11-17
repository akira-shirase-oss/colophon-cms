//! Locales on the published projection against real PostgreSQL (D-173, D-174): revisions pin
//! the tag their entry had, releases copy it and the translation group into publications, the
//! resolving read gives one row per group through a chain — inside the audience's own row
//! security, so a member-only translation is never an anonymous reader's fallback — and the
//! language list is replaced whole under its version and refuses to drop a tag in use. Needs
//! the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    PgPool, audience, changesets,
    delivery::{self, Audience, Resolution},
    entries::{self, Localized, NewRevision, SetLocale},
    locales::{self, Replaced},
    projects, releases,
    releases::Committed,
    scope::{self, ProjectTransaction},
};
use colophon_domain::locale::Language;
use common::TestDb;
use uuid::Uuid;

const HOUR: std::time::Duration = std::time::Duration::from_secs(3600);
const ARTICLES: &str = "articles";

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member")
}

/// A project whose owner may publish, with an open audience role `member`.
async fn project(db: &TestDb) -> (Uuid, Uuid) {
    let owner = db.administrator("owner").await;
    let project = projects::create(&db.app, owner, "site", "Site")
        .await
        .expect("created")
        .id;
    let mut tx = scope::begin_as_member(&db.app, owner, project)
        .await
        .expect("begin")
        .expect("a member");
    assert!(
        audience::create_role(&mut tx, "member", true)
            .await
            .expect("role")
    );
    tx.commit().await.expect("commit");
    (owner, project)
}

/// A revision of made-up content titled `title`, gated as `visibility`.
fn revision<'a>(
    id: Uuid,
    title: &'a str,
    text: &'a str,
    visibility: Option<&'a str>,
) -> NewRevision<'a> {
    NewRevision {
        id,
        content: text,
        content_hash: &[3; 32],
        title,
        slug: None,
        schema_version: 1,
        references: &[],
        assets: &[],
        unique: &[],
        visibility,
    }
}

/// Creates an entry titled `title` in `locale`; its id and revision.
async fn create(
    tx: &mut ProjectTransaction,
    title: &str,
    locale: Option<&str>,
    visibility: Option<&str>,
) -> (Uuid, Uuid) {
    let (entry, id) = (Uuid::now_v7(), Uuid::now_v7());
    let text = format!(r#"{{"title":"{title}"}}"#);
    entries::create(
        tx,
        entry,
        ARTICLES,
        revision(id, title, &text, visibility),
        locale,
    )
    .await
    .expect("created");
    (entry, id)
}

/// Adds `source`'s translation in `locale`, titled `title`; its id.
async fn translate(
    tx: &mut ProjectTransaction,
    source: Uuid,
    locale: &str,
    title: &str,
    visibility: Option<&str>,
) -> Uuid {
    let entry = Uuid::now_v7();
    let text = format!(r#"{{"title":"{title}"}}"#);
    let made = entries::create_localization(
        tx,
        entry,
        source,
        ARTICLES,
        locale,
        revision(Uuid::now_v7(), title, &text, visibility),
    )
    .await
    .expect("localized");
    assert!(matches!(made, Localized::Made(_)), "{made:?}");
    entry
}

/// Publishes the drafts of `entries` as one release; its generation.
async fn publish(tx: &mut ProjectTransaction, entries: &[Uuid]) -> i64 {
    let pins = changesets::pins(tx, entries).await.expect("pins");
    let changeset = Uuid::now_v7();
    changesets::create(tx, changeset, "release", &pins, &[5; 32], None)
        .await
        .expect("created");
    changesets::submit(tx, changeset).await.expect("submitted");
    changesets::lock(tx, changeset).await.expect("lock");
    changesets::review(tx, changeset, &[5; 32], "approve", "", None)
        .await
        .expect("approved");
    let review = changesets::reviews(tx, changeset, None)
        .await
        .expect("reviews")[0]
        .id;
    let base = releases::generation(tx).await.expect("generation").1;
    let Committed::Done { generation, .. } =
        releases::commit(tx, base, changeset, &[5; 32], review, HOUR)
            .await
            .expect("commit")
    else {
        panic!("the release commits");
    };
    generation
}

/// The revision's pinned tag.
async fn pinned(tx: &mut ProjectTransaction, revision: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT locale FROM revisions WHERE id = $1")
        .bind(revision)
        .fetch_one(&mut *tx.transaction)
        .await
        .expect("read")
}

/// `(title, locale)` of each row a resolving read of `articles` gives `audience`.
async fn read(
    db: &TestDb,
    project: Uuid,
    audience: &Audience,
    generation: i64,
    chain: &[&str],
    default: Option<&str>,
) -> Vec<(String, Option<String>)> {
    let mut tx = delivery::begin(&db.app, project, audience)
        .await
        .expect("begin");
    let environment = tx
        .environment("production")
        .await
        .expect("environment")
        .expect("found");
    let resolution = Resolution {
        chain: chain.iter().map(|tag| (*tag).to_owned()).collect(),
        default: default.map(str::to_owned),
    };
    let rows = tx
        .list_in_locale(environment.id, ARTICLES, generation, &resolution, None, 50)
        .await
        .expect("list");
    tx.finish().await.expect("finish");
    let mut seen: Vec<_> = rows
        .into_iter()
        .map(|row| (row.title, row.locale))
        .collect();
    seen.sort();
    seen
}

fn language(tag: &str, default: bool, fallback: &[&str]) -> Language {
    Language {
        tag: tag.to_owned(),
        label: tag.to_owned(),
        default,
        fallback: fallback.iter().map(|tag| (*tag).to_owned()).collect(),
    }
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn revisions_pin_the_tag_and_releases_copy_it_with_the_group() {
    let db = TestDb::new().await;
    let (owner, project) = project(&db).await;
    let mut tx = enter(&db.app, owner, project).await;

    // A source published before it had translations…
    let (english, first) = create(&mut tx, "Hello", Some("en"), None).await;
    let early = publish(&mut tx, &[english]).await;
    // …then translated: the group takes the source's own id, so its earlier publication is in
    // the same group as the translation's.
    let french = translate(&mut tx, english, "fr", "Bonjour", None).await;
    let later = publish(&mut tx, &[french]).await;
    // A draft whose tag changes after it was published: the next revision pins the new tag,
    // the publication keeps the old one until it is published again.
    let (moving, _) = create(&mut tx, "Moving", Some("de"), None).await;
    let moved_at = publish(&mut tx, &[moving]).await;
    assert_eq!(
        entries::set_locale(&mut tx, ARTICLES, moving, Some("it"))
            .await
            .expect("set"),
        SetLocale::Set
    );
    let draft = entries::get_with_live(&mut tx, ARTICLES, moving)
        .await
        .expect("get")
        .expect("exists")
        .0
        .revision
        .id;
    let save = Uuid::now_v7();
    let text = r#"{"title":"Moving on"}"#;
    entries::save(
        &mut tx,
        moving,
        draft,
        revision(save, "Moving on", text, None),
    )
    .await
    .expect("saved");
    assert_eq!(pinned(&mut tx, first).await.as_deref(), Some("en"));
    assert_eq!(
        pinned(&mut tx, save).await.as_deref(),
        Some("it"),
        "the save pins the moved tag"
    );

    let rows: Vec<(Uuid, Option<String>, Uuid, i64)> = sqlx::query_as(
        "SELECT entry_id, locale, group_key, valid_from FROM publications
         WHERE project_id = $1 ORDER BY valid_from, entry_id",
    )
    .bind(project)
    .fetch_all(&mut *tx.transaction)
    .await
    .expect("read");
    tx.commit().await.expect("commit");
    assert_eq!(
        rows,
        vec![
            (english, Some("en".to_owned()), english, early),
            (french, Some("fr".to_owned()), english, later),
            (moving, Some("de".to_owned()), moving, moved_at),
        ],
        "each publication names its revision's tag and its group"
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
#[expect(
    clippy::too_many_lines,
    reason = "one collection, read through every chain that matters"
)]
async fn a_resolving_read_gives_each_group_its_best_member_and_never_a_gated_fallback() {
    let db = TestDb::new().await;
    let (owner, project) = project(&db).await;
    let mut tx = enter(&db.app, owner, project).await;

    // Three documents: one in en, pt and pt-BR; one in en with a member-only fr; one untagged.
    let (home, _) = create(&mut tx, "Home", Some("en"), None).await;
    let home_pt = translate(&mut tx, home, "pt", "Início", None).await;
    let home_br = translate(&mut tx, home, "pt-BR", "Início (BR)", None).await;
    let (news, _) = create(&mut tx, "News", Some("en"), None).await;
    let news_fr = translate(&mut tx, news, "fr", "Nouvelles", Some("role:member")).await;
    let (plain, _) = create(&mut tx, "Plain", None, None).await;
    let generation = publish(&mut tx, &[home, home_pt, home_br, news, news_fr, plain]).await;
    tx.commit().await.expect("commit");

    let anonymous = Audience::Anonymous;
    let member = Audience::Member {
        role: "member".to_owned(),
    };
    let row = |title: &str, tag: Option<&str>| (title.to_owned(), tag.map(str::to_owned));

    // pt-BR → pt → en, en the default: Home in pt-BR, News and Plain (the default's) in en.
    assert_eq!(
        read(
            &db,
            project,
            &anonymous,
            generation,
            &["pt-BR", "pt", "en"],
            Some("en")
        )
        .await,
        vec![row("Home", None).0.clone()]
            .into_iter()
            .map(|_| row("Início (BR)", Some("pt-BR")))
            .chain([row("News", Some("en")), row("Plain", None)])
            .collect::<Vec<_>>()
    );
    // pt alone: one exact match; the untagged entry isn't pt.
    assert_eq!(
        read(&db, project, &anonymous, generation, &["pt"], Some("en")).await,
        vec![row("Início", Some("pt"))]
    );
    // The default alone takes the untagged entry too; without a default it doesn't.
    assert_eq!(
        read(&db, project, &anonymous, generation, &["en"], Some("en")).await,
        vec![
            row("Home", Some("en")),
            row("News", Some("en")),
            row("Plain", None)
        ]
    );
    assert_eq!(
        read(&db, project, &anonymous, generation, &["en"], None).await,
        vec![row("Home", Some("en")), row("News", Some("en"))]
    );
    // fr → en: the member gets the member-only French; the anonymous reader falls through to
    // the public English, never the French.
    assert_eq!(
        read(&db, project, &member, generation, &["fr", "en"], Some("en")).await,
        vec![
            row("Home", Some("en")),
            row("Nouvelles", Some("fr")),
            row("Plain", None)
        ]
    );
    assert_eq!(
        read(
            &db,
            project,
            &anonymous,
            generation,
            &["fr", "en"],
            Some("en")
        )
        .await,
        vec![
            row("Home", Some("en")),
            row("News", Some("en")),
            row("Plain", None)
        ]
    );
    // fr alone: nothing for the anonymous reader at all.
    assert_eq!(
        read(&db, project, &anonymous, generation, &["fr"], Some("en")).await,
        Vec::new()
    );

    // An entry's siblings, as the audience may see them.
    let mut tx = delivery::begin(&db.app, project, &anonymous)
        .await
        .expect("begin");
    let environment = tx
        .environment("production")
        .await
        .expect("environment")
        .expect("found");
    let group = tx
        .group_of(environment.id, ARTICLES, news)
        .await
        .expect("group")
        .expect("published");
    let siblings = tx
        .siblings(environment.id, ARTICLES, group, generation)
        .await
        .expect("siblings");
    assert_eq!(
        siblings
            .iter()
            .map(|sibling| sibling.entry_id)
            .collect::<Vec<_>>(),
        vec![news],
        "the gated translation isn't the anonymous reader's to see"
    );
    assert_eq!(
        tx.group_of(environment.id, ARTICLES, news_fr)
            .await
            .expect("group"),
        None,
        "nor is its group, through it"
    );
    tx.finish().await.expect("finish");
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_language_list_is_replaced_whole_under_its_version() {
    let db = TestDb::new().await;
    let (owner, project) = project(&db).await;
    let mut tx = enter(&db.app, owner, project).await;
    let empty = locales::policy(&mut tx).await.expect("policy");
    assert_eq!((empty.version, empty.languages.len()), (0, 0));
    assert_eq!(
        locales::configured(&mut tx, "pt-BR")
            .await
            .expect("configured"),
        None,
        "no list: any tag goes"
    );
    let list = [
        language("en", true, &[]),
        language("pt", false, &["en"]),
        language("pt-BR", false, &["pt", "en"]),
    ];
    assert_eq!(
        locales::replace(&mut tx, 0, &list).await.expect("replace"),
        Replaced::Replaced { version: 1 }
    );
    assert_eq!(
        locales::replace(&mut tx, 0, &list).await.expect("replace"),
        Replaced::Stale { current: 1 },
        "a replace that saw version 0 is stale now"
    );
    assert_eq!(
        locales::configured(&mut tx, "PT-br")
            .await
            .expect("configured"),
        Some(Some("pt-BR".to_owned())),
        "the list's own spelling"
    );
    assert_eq!(
        locales::configured(&mut tx, "fr")
            .await
            .expect("configured"),
        Some(None)
    );
    let read = locales::policy(&mut tx).await.expect("policy");
    assert_eq!(read.version, 1);
    assert_eq!(read.languages, list.to_vec());

    // Dropping a tag a live entry carries is refused, naming it; the refusal is rolled back.
    let (_, _) = create(&mut tx, "Olá", Some("pt-BR"), None).await;
    tx.commit().await.expect("commit");
    let mut tx = enter(&db.app, owner, project).await;
    match locales::replace(&mut tx, 1, &list[..2])
        .await
        .expect("replace")
    {
        Replaced::InUse {
            tag,
            total,
            entries,
        } => {
            assert_eq!((tag.as_str(), total, entries.len()), ("pt-BR", 1, 1));
            assert_eq!(entries[0].title, "Olá");
        }
        other => panic!("expected locale_in_use, got {other:?}"),
    }
    tx.transaction.rollback().await.expect("rollback");

    // Delivery's one statement answers the policy: listed spelling, chain, default, version.
    let environment = delivery::environment_in(&db.app, project, "production", Some("PT-BR"))
        .await
        .expect("looked up")
        .expect("found");
    assert_eq!(environment.locale_policy, 1);
    assert_eq!(environment.locales, ["en", "pt", "pt-BR"]);
    assert_eq!(environment.default_locale.as_deref(), Some("en"));
    assert_eq!(environment.listed_locale.as_deref(), Some("pt-BR"));
    assert_eq!(environment.chain, ["pt", "en"]);
    let unlisted = delivery::environment_in(&db.app, project, "production", Some("fr"))
        .await
        .expect("looked up")
        .expect("found");
    assert_eq!((unlisted.listed_locale, unlisted.chain.len()), (None, 0));
    db.drop().await;
}

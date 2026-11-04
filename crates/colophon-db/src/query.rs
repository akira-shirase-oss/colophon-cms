//! The query language's SQL translation (docs/modules/02-db.md, "Query translation"; DX.3,
//! D-103): a validated [`Query`] becomes one statement over each entry's *current draft*
//! revision — the draft perspective; published reads are a later step.
//!
//! A draft's content is canonical JSON kept as `text` (D1), so a field absent from it
//! compares as SQL `NULL`; each indexed kind casts to the Postgres type its spellings order
//! by — text-like kinds as the text they are, numbers as `numeric`, booleans as `boolean`,
//! dates, times and date-times as theirs. `eq` and `in` compare the stored JSON to the
//! canonical literal verbatim (`jsonb =`), so those values bind as-is; the ordered
//! operators compare the cast scalars; `contains` is a `strpos` search — single-line text
//! only, which [`Query::validate`] already guarantees. Compared values are bind
//! parameters, never interpolated — and field names are bound too (`->> $n` takes a text
//! parameter), even though `index: true` already narrowed them to declared ids.
//!
//! Sorting follows [`Query::order`]: the declared keys, then the entry id in the last
//! key's direction. `NULL` sorts last ascending and first descending (Postgres's default,
//! made explicit so the [`Position`] legs below agree with it). A position becomes the
//! lexicographic OR-of-ANDs a row-value tuple can't express across mixed directions: each
//! `same` leg an `IS NOT DISTINCT FROM` — so a `null` boundary matches the rows that don't
//! set the field — and each `after` leg the key's own direction, with `NULL`s placed where
//! the ordering puts them.

use std::{collections::HashMap, fmt::Write as _};

use colophon_domain::{
    query::{Direction, Filter, Operator, OrderKey, Position, Query},
    schema::{ContentType, Field, FieldKind, Refusal},
};
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use crate::{DbError, entries::Draft, scope::ProjectTransaction};

/// One page a query answered with.
#[derive(Debug)]
pub struct Page {
    /// The page's entries in the query's order: each current draft with the revision id
    /// and valid-from of the live publication it belongs to, if any.
    pub rows: Vec<(Draft, Option<(Uuid, i64)>)>,
    /// The sealed cursor for the page after this one, or `None` when this page is last.
    pub next_cursor: Option<String>,
}

/// Why a query couldn't run.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A cursor's position carries a value the field it sorts on couldn't hold: refused
    /// like a cursor that doesn't fit its query.
    #[error("{0}")]
    Cursor(Refusal),
    /// The database answered with an error.
    #[error("{0}")]
    Db(#[from] DbError),
}

/// Runs `query` against `collection`'s current drafts: the page's rows in the query's
/// order, and a cursor to the next page when a `limit + 1`th row answered. `after` is the
/// position a `?cursor=` decoded to; `created_by` scopes the page to one author's entries
/// for a role that may read only its own.
pub async fn page(
    tx: &mut ProjectTransaction,
    collection: &str,
    definition: &ContentType,
    query: &Query,
    after: Option<&Position>,
    created_by: Option<Uuid>,
) -> Result<Page, Error> {
    let statement = statement(
        tx.project,
        tx.environment,
        collection,
        definition,
        query,
        after,
        created_by,
    )?;
    // Audited above: the text is built only here, every value and field name a bind.
    let mut run = sqlx::query(sqlx::AssertSqlSafe(statement.text));
    for arg in statement.args {
        run = match arg {
            Arg::Id(value) => run.bind(value),
            Arg::OptId(value) => run.bind(value),
            Arg::Text(value) => run.bind(value),
            Arg::OptText(value) => run.bind(value),
            Arg::Rows(value) => run.bind(value),
        };
    }
    let rows = run
        .fetch_all(&mut *tx.transaction)
        .await
        .map_err(DbError::Query)?;
    let limit = usize::from(query.limit());
    let more = rows.len() > limit;
    let mut page_rows = Vec::with_capacity(limit);
    for row in rows.iter().take(limit) {
        page_rows.push((read(row)?, live(row)?));
    }
    let next_cursor = if more {
        let (last, _) = page_rows.last().ok_or(DbError::Corrupt(
            "a page says more without a row to continue from",
        ))?;
        let position = position_of(last, query).map_err(DbError::Corrupt)?;
        let token = query
            .cursor(&position)
            .map_err(|_| DbError::Corrupt("a page's own position doesn't fit the query"))?;
        Some(token)
    } else {
        None
    };
    Ok(Page {
        rows: page_rows,
        next_cursor,
    })
}

/// One bound value of a translated query, in `$1…` order.
#[derive(Debug, Clone, PartialEq)]
enum Arg {
    /// A uuid: the scope ids.
    Id(Uuid),
    /// A possibly-`NULL` uuid: the optional `created_by` scope.
    OptId(Option<Uuid>),
    /// Text: a field name, a compared literal, a `contains` needle, a boundary value.
    Text(String),
    /// Possibly-`NULL` text: a `null` boundary.
    OptText(Option<String>),
    /// The row cap.
    Rows(i64),
}

/// The statement a [`Query`] translates to: its SQL text and the values to bind, kept
/// apart so the translation can be read and tested without a database.
#[derive(Debug)]
struct Statement {
    text: String,
    args: Vec<Arg>,
}

/// Builds the statement for `query`: scope and own-entries predicates, one `AND` leg per
/// filter, the keyset predicate `after` decodes to, the ordering, and a `LIMIT` of
/// `limit + 1` so the page knows whether a next cursor is due. Compared values and field
/// names are arguments; a position value the field couldn't hold is refused like a cursor
/// that doesn't fit.
fn statement(
    project: Uuid,
    environment: Uuid,
    collection: &str,
    definition: &ContentType,
    query: &Query,
    after: Option<&Position>,
    created_by: Option<Uuid>,
) -> Result<Statement, Error> {
    let mut text = String::with_capacity(1024);
    let mut args: Vec<Arg> = Vec::new();
    // A field's bound-name parameter: each field binds once however often it is named.
    let mut fields: HashMap<&str, usize> = HashMap::new();
    args.extend([
        Arg::Id(project),
        Arg::Id(environment),
        Arg::Text(collection.to_owned()),
        Arg::OptId(created_by),
    ]);
    let order = query.order();
    // A position's field literals must be ones the field accepts, or the casts below
    // would turn a hand-made cursor into a database error. `null` — the field not set —
    // is always a boundary.
    if let Some(position) = &after {
        for ((key, _), literal) in order.iter().zip(&position.values) {
            match key {
                OrderKey::Field(field) if literal != "null" => {
                    fits(definition, field, literal)?;
                }
                OrderKey::EntryId => {
                    let id = scalar(literal);
                    Uuid::try_parse(&id).map_err(|_| {
                        refused(format!("the entry-id position '{id}' is not a uuid"))
                    })?;
                }
                OrderKey::Field(_) => {}
            }
        }
    }
    text.push_str(
        "SELECT e.id, e.collection, e.created_at, e.created_by, e.updated_at, e.locale,
         r.id AS revision_id, r.base_revision_id, r.author_id,
         r.created_at AS revision_created_at, r.content_hash, r.title, r.content,
         live.revision_id AS published_revision_id, live.valid_from AS published_valid_from
         FROM entries e
         CROSS JOIN LATERAL (
             SELECT r.id, r.base_revision_id, r.author_id, r.created_at, r.content_hash,
                    r.title, r.content
             FROM revisions r
             WHERE r.project_id = e.project_id
               AND r.environment_id = e.environment_id
               AND r.id = e.draft_revision_id
             LIMIT 1
         ) r
         CROSS JOIN LATERAL (SELECT r.content::jsonb AS body) d
         LEFT JOIN LATERAL (
             SELECT p.revision_id, p.valid_from
             FROM publications p
             WHERE p.project_id = e.project_id
               AND p.environment_id = e.environment_id
               AND p.entry_id = e.id
               AND p.valid_to IS NULL
             LIMIT 1
         ) live ON true
         WHERE e.project_id = $1 AND e.environment_id = $2 AND e.collection = $3
           AND ($4::uuid IS NULL OR e.created_by = $4)",
    );
    for filter in query.filters() {
        legs(&mut text, &mut args, &mut fields, definition, filter)?;
    }
    if let Some(position) = &after {
        keyset(
            &mut text,
            &mut args,
            &mut fields,
            definition,
            &order,
            position,
        )?;
    }
    text.push_str(" ORDER BY ");
    for (index, (key, direction)) in order.iter().enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        match key {
            OrderKey::Field(field_id) => {
                let field = declared(definition, field_id)?;
                let name = name_of(&mut args, &mut fields, field_id);
                let cast = cast(&field.kind);
                let (direction, nulls) = ordered(*direction);
                let _ = write!(text, "(d.body ->> ${name}){cast} {direction} NULLS {nulls}");
            }
            OrderKey::EntryId => {
                let (direction, _) = ordered(*direction);
                let _ = write!(text, "e.id {direction}");
            }
        }
    }
    let rows = args.len() + 1;
    args.push(Arg::Rows(i64::from(query.limit()) + 1));
    let _ = write!(text, " LIMIT ${rows}");
    Ok(Statement { text, args })
}

/// One filter's `AND` leg: the comparison its operator makes on the field's stored
/// values.
fn legs<'a>(
    text: &mut String,
    args: &mut Vec<Arg>,
    fields: &mut HashMap<&'a str, usize>,
    definition: &'a ContentType,
    filter: &'a Filter,
) -> Result<(), Error> {
    let name = name_of(args, fields, &filter.field);
    match filter.op {
        // `eq` and `in` compare the stored JSON to the canonical literal verbatim.
        Operator::Eq => {
            let value = push(args, Arg::Text(filter.value.clone()));
            let _ = write!(text, " AND (d.body -> ${name}) = ${value}::jsonb");
        }
        Operator::In => {
            let value = push(args, Arg::Text(filter.value.clone()));
            let _ = write!(
                text,
                " AND (d.body -> ${name}) IN (SELECT jsonb_array_elements(${value}::jsonb))"
            );
        }
        Operator::Contains => {
            let needle = push(args, Arg::Text(scalar(&filter.value)));
            let _ = write!(text, " AND strpos(d.body ->> ${name}, ${needle}) > 0");
        }
        op @ (Operator::Lt | Operator::Lte | Operator::Gt | Operator::Gte) => {
            let cast = cast(&declared(definition, &filter.field)?.kind);
            let value = push(args, Arg::Text(scalar(&filter.value)));
            let comparison = match op {
                Operator::Lt => "<",
                Operator::Lte => "<=",
                Operator::Gt => ">",
                Operator::Gte => ">=",
                _ => unreachable!("the ordered operators"),
            };
            let _ = write!(
                text,
                " AND (d.body ->> ${name}){cast} {comparison} ${value}{cast}"
            );
        }
    }
    Ok(())
}

/// The `AND (…)` one position adds: an OR-of-ANDs over the order's keys — the first key's
/// boundary, or that key equal and the next compared, and so on — so a mixed-direction
/// ordering gets the comparison a row-value tuple can't spell.
fn keyset<'a>(
    text: &mut String,
    args: &mut Vec<Arg>,
    fields: &mut HashMap<&'a str, usize>,
    definition: &'a ContentType,
    order: &'a [(OrderKey, Direction)],
    position: &Position,
) -> Result<(), Error> {
    text.push_str(" AND (");
    for (index, ((key, direction), literal)) in order.iter().zip(&position.values).enumerate() {
        if index > 0 {
            text.push_str(" OR ");
        }
        text.push('(');
        for ((earlier, _), earlier_literal) in order.iter().zip(&position.values).take(index) {
            let leg = same(args, fields, definition, earlier, earlier_literal)?;
            let _ = write!(text, "{leg} AND ");
        }
        let leg = beyond(args, fields, definition, key, literal, *direction)?;
        text.push_str(&leg);
        text.push(')');
    }
    text.push(')');
    Ok(())
}

/// The `same` leg of a keyset term: `expr` equals the boundary's value — `null`s
/// included, so `IS NOT DISTINCT FROM` and never a bare `=`.
fn same<'a>(
    args: &mut Vec<Arg>,
    fields: &mut HashMap<&'a str, usize>,
    definition: &'a ContentType,
    key: &'a OrderKey,
    literal: &str,
) -> Result<String, Error> {
    match key {
        OrderKey::Field(field_id) => {
            let name = name_of(args, fields, field_id);
            let cast = cast(&declared(definition, field_id)?.kind);
            let value = boundary(args, literal);
            Ok(format!(
                "(d.body ->> ${name}){cast} IS NOT DISTINCT FROM ${value}{cast}"
            ))
        }
        OrderKey::EntryId => {
            let value = boundary(args, literal);
            Ok(format!("e.id = ${value}::uuid"))
        }
    }
}

/// The `after` leg of a keyset term: `expr` strictly past the boundary in the key's own
/// direction — with `NULL`s placed where the ordering puts them, so a `null` boundary is
/// the end ascending and only `NULL`s stay before it descending.
fn beyond<'a>(
    args: &mut Vec<Arg>,
    fields: &mut HashMap<&'a str, usize>,
    definition: &'a ContentType,
    key: &'a OrderKey,
    literal: &str,
    direction: Direction,
) -> Result<String, Error> {
    match key {
        OrderKey::Field(field_id) => {
            let name = name_of(args, fields, field_id);
            let cast = cast(&declared(definition, field_id)?.kind);
            let value = boundary(args, literal);
            let expr = format!("(d.body ->> ${name}){cast}");
            // Ascending puts `NULL` last — past every non-null boundary, nowhere past a
            // `null` one; descending puts it first — the mirror.
            // The parens matter: a `same AND (X OR Y)` term, not `same AND X OR Y`.
            let leg = match direction {
                Direction::Asc => format!(
                    "({expr} > ${value}{cast} OR ({expr} IS NULL AND ${value} IS NOT NULL))"
                ),
                Direction::Desc => format!(
                    "({expr} < ${value}{cast} OR ({expr} IS NOT NULL AND ${value} IS NULL))"
                ),
            };
            Ok(leg)
        }
        OrderKey::EntryId => {
            let value = boundary(args, literal);
            let comparison = match direction {
                Direction::Asc => ">",
                Direction::Desc => "<",
            };
            Ok(format!("e.id {comparison} ${value}::uuid"))
        }
    }
}

/// One argument's place: `$n` counting from `$1`.
fn push(args: &mut Vec<Arg>, arg: Arg) -> usize {
    args.push(arg);
    args.len()
}

/// A field's bound-name parameter, allocated once per statement.
fn name_of<'a>(args: &mut Vec<Arg>, fields: &mut HashMap<&'a str, usize>, id: &'a str) -> usize {
    if let Some(&at) = fields.get(id) {
        at
    } else {
        let at = push(args, Arg::Text(id.to_owned()));
        fields.insert(id, at);
        at
    }
}

/// A position literal as a bindable value: `null` as `NULL`, a string's contents, a
/// number's or boolean's own spelling.
fn boundary(args: &mut Vec<Arg>, literal: &str) -> usize {
    let value = match serde_json::from_str::<Value>(literal) {
        Ok(Value::Null) => None,
        Ok(Value::String(text)) => Some(text),
        _ => Some(literal.to_owned()),
    };
    push(args, Arg::OptText(value))
}

/// The plain text of a canonical literal: a string's contents, any other scalar's own
/// spelling.
fn scalar(literal: &str) -> String {
    match serde_json::from_str::<Value>(literal) {
        Ok(Value::String(text)) => text,
        _ => literal.to_owned(),
    }
}

/// The Postgres type an indexed kind's spellings order by, or nothing when they compare
/// as the text they're stored as.
const fn cast(kind: &FieldKind) -> &'static str {
    match kind {
        FieldKind::Number { .. } => "::numeric",
        FieldKind::Boolean => "::boolean",
        FieldKind::Date => "::date",
        FieldKind::Time => "::time",
        FieldKind::DateTime => "::timestamptz",
        _ => "",
    }
}

/// A direction's spelling, and where its `NULL`s land — last ascending, first
/// descending, the same orderings the `after` legs are written against.
const fn ordered(direction: Direction) -> (&'static str, &'static str) {
    match direction {
        Direction::Asc => ("ASC", "LAST"),
        Direction::Desc => ("DESC", "FIRST"),
    }
}

/// The field `id` names — every one a query can mention is declared and `index`ed,
/// because [`Query::validate`] ran before this.
fn declared<'a>(definition: &'a ContentType, id: &str) -> Result<&'a Field, Error> {
    definition
        .fields()
        .iter()
        .find(|field| field.id == id)
        .ok_or_else(|| DbError::Corrupt("a validated query names an undeclared field").into())
}

/// Whether a position's `literal` is a value `field` could hold — checked the way the
/// field's own filter values are, so a cursor carrying another kind's spelling is refused
/// instead of failing a cast. The domain's [`Position`] check is structural; whether a
/// field accepts the value is this layer's compare.
fn fits(definition: &ContentType, field: &str, literal: &str) -> Result<(), Error> {
    let value: Value =
        serde_json::from_str(literal).map_err(|_| refused(format!("'{literal}' is not JSON")))?;
    let probe = serde_json::json!({"filters": [{"field": field, "op": "eq", "value": value}]});
    Query::validate(definition, &probe)
        .map(|_| ())
        .map_err(|_| {
            refused(format!(
                "the position's {field} value {literal} is not one the field accepts"
            ))
        })
}

/// The `position_mismatch` a wrong position carries, like the cursor checks' own.
fn refused(message: String) -> Error {
    Error::Cursor(Refusal {
        code: "position_mismatch",
        pointer: String::new(),
        message,
    })
}

/// One row of the page's `SELECT` as the `Draft` the API's views take — the same columns
/// [`crate::entries::get_with_live`] reads.
fn read(row: &sqlx::postgres::PgRow) -> Result<Draft, DbError> {
    Ok(Draft {
        entry_id: row.try_get("id").map_err(DbError::Query)?,
        collection: row.try_get("collection").map_err(DbError::Query)?,
        created_at: row.try_get("created_at").map_err(DbError::Query)?,
        created_by: row.try_get("created_by").map_err(DbError::Query)?,
        updated_at: row.try_get("updated_at").map_err(DbError::Query)?,
        revision: crate::entries::Revision {
            id: row.try_get("revision_id").map_err(DbError::Query)?,
            base_revision_id: row.try_get("base_revision_id").map_err(DbError::Query)?,
            author_id: row.try_get("author_id").map_err(DbError::Query)?,
            created_at: row.try_get("revision_created_at").map_err(DbError::Query)?,
            content_hash: row.try_get("content_hash").map_err(DbError::Query)?,
            title: row.try_get("title").map_err(DbError::Query)?,
        },
        content: row.try_get("content").map_err(DbError::Query)?,
        locale: row.try_get("locale").map_err(DbError::Query)?,
    })
}

/// The live publication half of one row, if it is published.
fn live(row: &sqlx::postgres::PgRow) -> Result<Option<(Uuid, i64)>, DbError> {
    let revision: Option<Uuid> = row
        .try_get("published_revision_id")
        .map_err(DbError::Query)?;
    Ok(match revision {
        Some(revision) => Some((
            revision,
            row.try_get("published_valid_from")
                .map_err(DbError::Query)?,
        )),
        None => None,
    })
}

/// The position the page's last row stands on: one canonical literal per order key —
/// `null` where the entry doesn't set the field — ready for [`Query::cursor`] to seal.
fn position_of(draft: &Draft, query: &Query) -> Result<Position, &'static str> {
    let document: Value =
        serde_json::from_str(&draft.content).map_err(|_| "a draft's stored content isn't JSON")?;
    let mut values = Vec::with_capacity(query.order().len());
    for (key, _) in query.order() {
        values.push(match key {
            OrderKey::Field(field) => match document.get(field.as_str()) {
                Some(value) => serde_json::to_string(value)
                    .map_err(|_| "a stored field doesn't write as JSON")?,
                None => "null".to_owned(),
            },
            OrderKey::EntryId => serde_json::to_string(&draft.entry_id.to_string())
                .map_err(|_| "an entry id doesn't write as JSON")?,
        });
    }
    Ok(Position { values })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const PROJECT: Uuid = Uuid::from_u128(1);
    const ENVIRONMENT: Uuid = Uuid::from_u128(2);
    const ENTRY: &str = "123e4567-e89b-12d3-a456-426614174000";

    /// Every kind the translation casts differently, on one type.
    fn definition() -> ContentType {
        ContentType::parse(&json!({
            "name": "Posts", "title_field": "headline", "fields": [
                {"id": "headline", "label": "Headline", "type": "text", "required": true, "max_chars": 120, "index": true},
                {"id": "author", "label": "Author", "type": "reference", "to": "staff", "index": true},
                {"id": "publish_at", "label": "Publish at", "type": "datetime", "index": true},
                {"id": "section", "label": "Section", "type": "enum", "values": ["News", "Guides"], "index": true},
                {"id": "rating", "label": "Rating", "type": "number", "integer": true, "min": 0, "max": 5, "index": true},
                {"id": "featured", "label": "Featured", "type": "boolean", "index": true},
                {"id": "slug", "label": "Slug", "type": "slug", "index": true},
                {"id": "born", "label": "Born", "type": "date", "index": true},
                {"id": "opens", "label": "Opens", "type": "time", "index": true}
            ]
        }))
        .expect("a valid definition")
    }

    fn checked(value: &Value) -> Query {
        Query::validate(&definition(), value).expect("a valid query")
    }

    /// The statement a document builds, or the refusal.
    fn built(value: &Value, after: Option<&Position>) -> Result<Statement, Error> {
        let query = checked(value);
        statement(
            PROJECT,
            ENVIRONMENT,
            "posts",
            &definition(),
            &query,
            after,
            None,
        )
    }

    /// A position of canonical literals.
    fn boundary(values: &[&str]) -> Position {
        Position {
            values: values.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    /// The part after the last `WHERE` atom: filters' and the keyset's legs.
    fn predicate(text: &str) -> &str {
        let (where_part, _) = text.rsplit_once(" ORDER BY ").expect("the order");
        where_part
            .split("e.created_by = $4)")
            .nth(1)
            .expect("the scope predicate")
    }

    /// The `AND` leg one filter builds.
    fn leg(field: &str, op: &str, value: &Value) -> (String, Vec<Arg>) {
        let stmt = built(
            &json!({"filters": [{"field": field, "op": op, "value": value}]}),
            None,
        )
        .expect("builds");
        (predicate(&stmt.text).to_owned(), stmt.args)
    }

    #[test]
    fn every_operator_gets_its_comparison_and_every_value_binds() {
        let cases = [
            (
                ("headline", "eq", json!("Hi")),
                " AND (d.body -> $5) = $6::jsonb",
                Arg::Text("\"Hi\"".to_owned()),
            ),
            (
                ("section", "in", json!(["News", "Guides"])),
                " AND (d.body -> $5) IN (SELECT jsonb_array_elements($6::jsonb))",
                Arg::Text("[\"Guides\",\"News\"]".to_owned()),
            ),
            (
                ("headline", "contains", json!("the \"mark\"")),
                " AND strpos(d.body ->> $5, $6) > 0",
                // The needle, without its quotes.
                Arg::Text("the \"mark\"".to_owned()),
            ),
            (
                ("rating", "lt", json!(3)),
                " AND (d.body ->> $5)::numeric < $6::numeric",
                Arg::Text("3".to_owned()),
            ),
            (
                ("born", "gte", json!("2000-02-29")),
                " AND (d.body ->> $5)::date >= $6::date",
                Arg::Text("2000-02-29".to_owned()),
            ),
            (
                ("opens", "lte", json!("14:30")),
                " AND (d.body ->> $5)::time <= $6::time",
                Arg::Text("14:30".to_owned()),
            ),
            (
                ("publish_at", "gt", json!("2026-10-25T14:30:00Z")),
                " AND (d.body ->> $5)::timestamptz > $6::timestamptz",
                Arg::Text("2026-10-25T14:30:00Z".to_owned()),
            ),
            (
                ("featured", "eq", json!(true)),
                " AND (d.body -> $5) = $6::jsonb",
                Arg::Text("true".to_owned()),
            ),
            (
                ("slug", "gte", json!("my-post")),
                " AND (d.body ->> $5) >= $6",
                Arg::Text("my-post".to_owned()),
            ),
        ];
        for ((field, op, value), expected, bound) in cases {
            let (text, args) = leg(field, op, &value);
            assert_eq!(text, expected, "{field} {op}");
            assert_eq!(
                args[4..],
                [Arg::Text(field.to_owned()), bound, Arg::Rows(21)],
                "{field} {op}"
            );
        }
        // The SQL text never holds a literal: everything a user sent went in a bind.
        let stmt = built(
            &json!({"filters": [{"field": "headline", "op": "contains", "value": "x; DROP TABLE y"}]}),
            None,
        )
        .expect("builds");
        assert!(!stmt.text.contains('\''), "{}", stmt.text);
        // And a field named twice binds its name once.
        let twice = built(
            &json!({
                "filters": [{"field": "headline", "op": "contains", "value": "x"}],
                "sort": [{"field": "headline"}]
            }),
            None,
        )
        .expect("builds");
        let names = twice
            .args
            .iter()
            .filter(|arg| **arg == Arg::Text("headline".to_owned()))
            .count();
        assert_eq!(names, 1);
    }

    #[test]
    fn the_order_is_the_declared_keys_then_the_entry_id() {
        let stmt = built(
            &json!({"sort": [{"field": "publish_at", "dir": "desc"}, {"field": "rating"}]}),
            None,
        )
        .expect("builds");
        let order = stmt.text.rsplit_once(" ORDER BY ").expect("order").1;
        assert_eq!(
            order,
            concat!(
                "(d.body ->> $5)::timestamptz DESC NULLS FIRST, ",
                "(d.body ->> $6)::numeric ASC NULLS LAST, e.id ASC LIMIT $7"
            )
        );
        // No sort: just the entry id, ascending.
        let bare = built(&json!({}), None).expect("builds");
        assert_eq!(
            bare.text.rsplit_once(" ORDER BY ").expect("order").1,
            "e.id ASC LIMIT $5"
        );
    }

    #[test]
    fn a_position_is_the_lexicographic_or_of_ands() {
        let id = format!("\"{ENTRY}\"");
        let position = boundary(&["\"2026-10-25T14:30:00Z\"", "4", &id]);
        let stmt = built(
            &json!({"sort": [{"field": "publish_at", "dir": "desc"}, {"field": "rating"}]}),
            Some(&position),
        )
        .expect("builds");
        // First key compared its own way; then equal on it and the second key compared;
        // then equal on both and the entry id compared. Every `same` is
        // IS NOT DISTINCT FROM, so a `null` boundary matches field-less rows.
        assert_eq!(
            predicate(&stmt.text),
            concat!(
                " AND ((((d.body ->> $5)::timestamptz < $6::timestamptz OR ",
                "((d.body ->> $5)::timestamptz IS NOT NULL AND $6 IS NULL))) ",
                "OR ((d.body ->> $5)::timestamptz IS NOT DISTINCT FROM $7::timestamptz AND ",
                "((d.body ->> $8)::numeric > $9::numeric OR ",
                "((d.body ->> $8)::numeric IS NULL AND $9 IS NOT NULL))) ",
                "OR ((d.body ->> $5)::timestamptz IS NOT DISTINCT FROM $10::timestamptz AND ",
                "(d.body ->> $8)::numeric IS NOT DISTINCT FROM $11::numeric AND ",
                "e.id > $12::uuid))"
            )
        );
        // The boundary's values are bound, in order: the datetime twice, the number
        // twice, the uuid once.
        assert_eq!(
            stmt.args[4..stmt.args.len() - 1],
            [
                Arg::Text("publish_at".to_owned()),
                Arg::OptText(Some("2026-10-25T14:30:00Z".to_owned())),
                Arg::OptText(Some("2026-10-25T14:30:00Z".to_owned())),
                Arg::Text("rating".to_owned()),
                Arg::OptText(Some("4".to_owned())),
                Arg::OptText(Some("2026-10-25T14:30:00Z".to_owned())),
                Arg::OptText(Some("4".to_owned())),
                Arg::OptText(Some(ENTRY.to_owned())),
            ]
        );
        assert_eq!(stmt.args.last(), Some(&Arg::Rows(21)));
    }

    #[test]
    fn a_null_boundary_is_bound_not_spelled() {
        let id = format!("\"{ENTRY}\"");
        let position = boundary(&["null", &id]);
        let stmt =
            built(&json!({"sort": [{"field": "headline"}]}), Some(&position)).expect("builds");
        assert_eq!(
            predicate(&stmt.text),
            concat!(
                " AND ((((d.body ->> $5) > $6 OR ((d.body ->> $5) IS NULL AND $6 IS NOT NULL))) ",
                "OR ((d.body ->> $5) IS NOT DISTINCT FROM $7 AND e.id > $8::uuid))"
            )
        );
        assert_eq!(stmt.args[5], Arg::OptText(None));
        assert_eq!(stmt.args[6], Arg::OptText(None));
        assert_eq!(stmt.args[7], Arg::OptText(Some(ENTRY.to_owned())));
    }

    #[test]
    fn a_position_the_field_cant_hold_is_refused() {
        let id = format!("\"{ENTRY}\"");
        // `8` is a fine number but not one `rating` (0 to 5) accepts; `"soon"` is a fine
        // string but not one a date casts.
        for position in [boundary(&["8", &id]), boundary(&["\"soon\"", &id])] {
            let document = match position.values[0].as_str() {
                "8" => json!({"sort": [{"field": "rating"}]}),
                _ => json!({"sort": [{"field": "born"}]}),
            };
            match built(&document, Some(&position)) {
                Err(Error::Cursor(refusal)) => {
                    assert_eq!(refusal.code, "position_mismatch");
                }
                other => panic!("expected a refusal, got {other:?}"),
            }
        }
        // And the entry id must be a uuid.
        let bad = boundary(&["\"2026-10-25T14:30:00Z\"", "\"not-a-uuid\""]);
        match built(
            &json!({"sort": [{"field": "publish_at", "dir": "desc"}]}),
            Some(&bad),
        ) {
            Err(Error::Cursor(refusal)) => assert_eq!(refusal.code, "position_mismatch"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
}

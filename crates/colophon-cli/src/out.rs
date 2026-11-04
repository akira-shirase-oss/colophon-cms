//! The machine-readable output contract (docs/modules/08-cli.md, "Output"; D-119).
//!
//! `--json` prints exactly one object on stdout; `--jsonl`, the list commands' streaming
//! form, prints one object per line and follows every page. Both are the API's
//! `data`/`error` envelope behind the contract's version and the command's name:
//! `{"version": 2, "command": "content list", "data": …}` on success, the same object
//! with `error` in place of `data` on failure. A refusal the API sent arrives whole
//! (`code`, `message`, `details`, `request_id`); a failure the CLI raised names the
//! exit class. Human output and `error: …` diagnostics stay as they were — a text
//! run's errors still never touch stdout.

use serde_json::{Value, json};

use crate::client::Failure;

/// The contract's version: 1 was the flat `{"version": 1, "command": …, …fields}` shape
/// (D-041, D-106); 2 wraps the result in the API's `data`/`error` envelope.
pub(crate) const VERSION: u32 = 2;

/// What stdout carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    Text,
    Json,
    Jsonl,
}

impl Format {
    /// The format `--json` and `--jsonl` pick; they conflict in clap, so `jsonl` only
    /// decides a tie that can't happen.
    pub(crate) fn of(json: bool, jsonl: bool) -> Self {
        if jsonl {
            Self::Jsonl
        } else if json {
            Self::Json
        } else {
            Self::Text
        }
    }

    /// Whether stdout is machine-readable.
    pub(crate) fn machine(self) -> bool {
        !matches!(self, Self::Text)
    }
}

/// The envelope a command's machine output travels in. `command` is the name the
/// `command` field reports, as `colophon <command>` spells it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Out {
    command: &'static str,
    /// The format stdout uses.
    pub(crate) format: Format,
}

impl Out {
    pub(crate) const fn new(command: &'static str, format: Format) -> Self {
        Self { command, format }
    }

    /// `{"version": 2, "command": …, "data": data}` on stdout — the `--json` result.
    pub(crate) fn print_data(&self, data: &Value) {
        println!("{}", self.wrapped("data", data));
    }

    /// One item's `{"version": 2, "command": …, "data": item}` — a `--jsonl` line.
    pub(crate) fn print_item(&self, item: &Value) {
        println!("{}", self.wrapped("data", item));
    }

    /// `{"version": 2, "command": …, "error": error}` on stdout — only on a machine
    /// format, so a text run's failure stays on stderr alone.
    pub(crate) fn print_error(&self, error: &Value) {
        if self.format.machine() {
            println!("{}", self.wrapped("error", error));
        }
    }

    /// A [`Failure`]'s envelope, printed.
    pub(crate) fn print_failure(&self, failure: &Failure) {
        self.print_error(&failure.envelope());
    }

    fn wrapped(&self, key: &str, payload: &Value) -> Value {
        json!({"version": VERSION, "command": self.command, key: payload})
    }
}

/// The `{"code", "message"}` object a failure the CLI itself raised gets; `code` is the
/// exit class, so a machine can tell `usage` from `unavailable` without parsing words.
pub(crate) fn cli_error(code: &'static str, message: &str) -> Value {
    json!({"code": code, "message": message})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Exit;

    #[test]
    fn the_envelope_is_versioned_and_named() {
        let out = Out::new("content list", Format::Json);
        assert_eq!(
            out.wrapped("data", &json!({"entries": []})),
            json!({"version": 2, "command": "content list", "data": {"entries": []}})
        );
    }

    #[test]
    fn flags_pick_one_format() {
        assert_eq!(Format::of(false, false), Format::Text);
        assert_eq!(Format::of(true, false), Format::Json);
        assert_eq!(Format::of(false, true), Format::Jsonl);
        assert_eq!(Format::of(true, true), Format::Jsonl);
        assert!(!Format::Text.machine());
        assert!(Format::Json.machine() && Format::Jsonl.machine());
    }

    #[test]
    fn an_api_refusal_arrives_whole_and_a_cli_failure_names_its_class() {
        let api = Failure {
            exit: Exit::Denied,
            message: "no write access (at /title) [HTTP 403]".to_owned(),
            refusal: Some(json!({
                "code": "forbidden", "message": "no write access",
                "details": {"reason": "field_not_permitted", "pointer": "/title"},
                "request_id": "req-1"
            })),
        };
        assert_eq!(
            api.envelope()["details"]["reason"],
            "field_not_permitted",
            "the API's details travel verbatim"
        );
        let own = Failure::new(Exit::Usage, "the document is not JSON");
        assert_eq!(
            own.envelope(),
            json!({"code": "usage", "message": "the document is not JSON"})
        );
    }
}

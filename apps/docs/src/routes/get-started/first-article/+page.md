<script>
	import Status from '#lib/components/Status.svelte';
</script>

An **entry** is one piece of content, such as an article, in a **collection** (all the entries of one content type). This page writes one article in the Studio and one from the command line, and shows that both are the same kind of record: every save becomes a new **version** that never changes afterwards. It assumes the project from [Your first project](/get-started/first-project/).

## Write an article in the Studio

1. In the sidebar, under **Content**, open **Articles**. An empty collection says *Nothing here yet*.
2. Press **New entry**. The dialog asks only for the fields that have no sensible starting value, here the **Title**, and says *It starts as a draft. Nothing is published until a changeset with it is approved and published.*
3. Type a title and press **Create**. The entry opens.
4. Write in the **Body**. The toolbar has headings, lists, quotes, code, callouts and links.

You don't save by hand. A second after you stop typing, the Studio saves; **Save now** (⌘S, or Ctrl+S on Windows and Linux) saves at once. The panel beside the fields says where your words are, and it says **Saved** only once the server has stored them. Until then your edit is kept in this browser, so a closed tab, a dropped network or a restarted server loses nothing. [Writing and saving](/editors/writing/) explains every message, and what happens when two people edit the same article.

Back in **Articles**, the entry is listed with the state **Draft**: readers can't see it until it is published.

## Every save is a version

Open the entry's **History** tab. Each save is listed, newest first, with its title, when it was saved and the start of its fingerprint (a SHA-256 hash of its content). The newest is marked **Current draft**. Press **Compare with draft** on an older one to see what changed, word by word. Versions are never edited or deleted. [Versions and comparing](/editors/versions/) has more.

## Write one from the command line

After [signing the CLI in](/get-started/quickstart/#7-use-the-command-line), create an article. Its content is JSON with the same fields: a title, and a body in the [rich-text format](/reference/rich-text/):

```sh runnable
cargo run --release -p colophon-cli -- content create articles --project my-site --data '{"title":"Hello from the terminal","body":{"schema":"colophon/rich-text","version":1,"doc":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Written with the colophon CLI."}]}]}}}'
```

It prints `Created articles` followed by the entry's id and its title. `--file article.json` reads the content from a file instead of `--data`; `--json` prints one versioned JSON result instead of text. List the collection, newest first:

```sh runnable
cargo run --release -p colophon-cli -- content list articles --project my-site
```

Each line is an entry's id, when it was last saved and its title. The new article is in the Studio at once, with the same states and history: the CLI calls the same API, with the same checks, as the Studio.

If an attempt gets no answer, the CLI retries with the same **idempotency key**, so the server can tell it is a repeat and never creates the entry twice; if every attempt fails, it prints the key, and running the command again with `--idempotency-key` and that key is equally safe. Content that doesn't fit the content type exits with code 2 and the server's reason, a missing permission with 3. [Command line](/reference/cli/) lists every exit code.

## What the server checks

The server checks every save against the collection's **content type**, whoever sends it. For Articles, the title is required, on one line, and at most 200 characters, and the body must be valid rich text: a document the format allows, within its size and nesting limits, checked in Rust. Anything else is refused with a code and a pointer to the field, and nothing is stored.

## Content types, briefly

A content type is a versioned list of fields. Today a field is text, rich text, number, yes/no, choice or date-time, each with its own limits, and a change to a content type is allowed only when it can't make stored entries invalid. Developers and owners create content types through the API (`POST` to the project's `content-types`; see [Content types](/reference/api/content-types/)). <Status done="1.5" />

On the step 3.6 branch, owners and developers build content types in the Studio, under **Settings → Content types**, and a change is checked against the entries actually stored: a refusal lists the entries in the way. <Status building="3.6" /> [Content types and URL names](/concepts/content-types/) explains both.

## URL names

A **URL name** is the part of a web address that names one article, such as `lemon-tart`. It arrives with a new field type on the 3.6 branch: a button beside the field makes one from the title, and two entries in one collection can't share one. <Status building="3.6" /> [URL names](/editors/url-names/) has the rules.

## Next

[Your first publish and receipt](/get-started/first-publish/).

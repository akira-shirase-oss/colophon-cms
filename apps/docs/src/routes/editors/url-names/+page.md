<script>
	import Status from '#lib/components/Status.svelte';
</script>

A **URL name** is the part of a web address that names one article: `lemon-tart` in `example.com/articles/lemon-tart/`. It is short, readable, and stays the same when you fix a typo in the title. This page describes step 3.6, which is being built on its own branch and isn't in the released code yet: in today's Studio, articles have no URL name, and the reference website addresses each article by its long id.

## Where you find it

An article has a URL name only when its content type has a field for one. Whoever manages content types (an owner or a developer) adds it under **Settings → Content types**, as a field of the type **Slug**, usually labelled *URL name*. A content type has at most one. The built-in Articles type doesn't have one until someone adds it.

In the article, the field sits with the others, with the hint *Lowercase letters, digits and hyphens; unique in this collection.* It is optional unless whoever added it made it required.

## Making one from the title

Beside the field is a button named after the field it copies from, usually **From Title**. Press it and the Studio fills in a URL name made from the title:

- accents are dropped, so *Crème brûlée, at home* becomes `creme-brulee-at-home`;
- capitals become lower case;
- every run of spaces, punctuation and other characters becomes a single hyphen, and none is left at either end;
- it is cut to 120 characters.

The button doesn't follow the title as you type; you press it when you want it, and it is greyed out when the URL name already matches the title, or when the title has nothing it can use (a title only in Japanese script, for example). You can always type or change the URL name yourself.

The Studio saves it with the rest of the article, as it saves everything ([Writing and saving](/editors/writing/)).

## The rules

A URL name is 1 to 120 characters: lowercase letters a to z and digits, in groups joined by single hyphens.

| Allowed | Refused, and why |
|---|---|
| `lemon-tart` | `Lemon-Tart`: capital letters |
| `2026-plans` | `lemon tart`: a space |
| `a` | `lemon--tart`: two hyphens in a row |
| `faq-3` | `-lemon` or `lemon-`: a hyphen at the start or end |
| | `lemon_tart`, `crème`: anything other than a to z, 0 to 9 and hyphens |

Colophon refuses a URL name that breaks a rule instead of quietly correcting it, so what is stored is exactly what you saw. The save message then says, for a field labelled *URL name*: *Not saved: URL name is 1 to 120 lowercase letters and digits, in groups joined by single hyphens, like my-first-post.* Fix it, or press **From Title**.

## Two articles can't share one

Within one collection, a URL name belongs to one article. Another article can't take it while the first one has it, either in its draft or live on the website. The save is refused, and the message (which still uses the old word, *slug*) says which kind:

| Message | What it means | What to do |
|---|---|---|
| *Not saved: "lemon-tart" is already the slug of another entry (draft)* | Another article's draft uses it. | Choose another, or change the other article's URL name first. |
| *Not saved: "lemon-tart" is already the slug of another entry (published)* | Another article is live with it. | Choose another. To move the name from one article to the other, change the live one's URL name and publish that first. |

The message doesn't name the other article yet; **Search titles** in the collection helps you find it.

Two different collections may use the same URL name: an article and a recipe can both be `lemon-tart`. An article taken down from the website frees its live URL name, but its draft keeps it.

If two people race for the same name, the database decides: one save wins, and the other is refused with *another entry in this collection has the same slug, as a draft or published*. If a publish request would put a URL name live while another article still has it live, the whole publish is refused with *an entry in this changeset has a slug that another entry still publishes; publish that entry's new slug first, or in the same changeset*. Do what it says: publish the other article's change first, or add it to the same request.

## Changing a URL name later

You can change an article's URL name like any other field; the new one goes live with the next release. The old address then stops working: automatic redirects from old URL names aren't built yet. <Status planned="P4" /> Choose URL names you won't need to change.

## Technical details

The field type is `slug`, with an optional `source` naming the single-line text field the Studio suggests it from; the server never derives one. A value that breaks the rule is `422 validation_failed` with the reason `invalid_slug`; a taken one is `409 conflict` with the reason `slug_taken`, and the error's details name the entry in the way. Two partial unique indexes in PostgreSQL (migration 0016) are the rule, one among drafts and one among open publications, so a precheck that loses a race can't let a duplicate through. The delivery API includes each entry's `slug` when it has one, and the reference website uses it for the article's address. Decision D-054, on the [3.6 branch](https://github.com/akira-shirase-oss/colophon-cms/blob/p3/3.6-modelling/docs/DECISIONS.md); [Content types and URL names](/concepts/content-types/) has the rest.

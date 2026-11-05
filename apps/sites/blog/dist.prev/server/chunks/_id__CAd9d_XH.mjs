import { t as __exportAll } from "./rolldown-runtime_782esTHO.mjs";
import { b as createAstro, l as renderTemplate, r as renderComponent, u as maybeRenderHead, v as unescapeHTML, x as createComponent } from "./server_BrZnmBTE.mjs";
import "./compiler_By5Sh0rm.mjs";
import { a as site, r as sessionToken } from "./session_Br3_L6V2.mjs";
import { n as entry, r as mediaUrl } from "./delivery_CYqFMhoW.mjs";
import { t as me } from "./audience_zyCBDry1.mjs";
import { t as $$Base } from "./Base_BTcjQ9f7.mjs";
var HEADING_LEVELS = [
	2,
	3,
	4
];
var CALLOUT_TONES = [
	"note",
	"tip",
	"warning"
];
var LIMITS = {
	maxDepth: 16,
	maxBytes: 262144,
	maxNodes: 1e4,
	maxAltLength: 300,
	maxHrefLength: 2048
};
var CODE_LANGUAGE = /^[a-z0-9][a-z0-9+#.-]{0,19}$/;
var ASSET_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
/**
* https:, http: and mailto: URLs; paths starting with exactly one `/`; fragments starting with
* `#`. Control characters, whitespace and backslashes are refused anywhere: browsers strip tabs
* and newlines and read `\` as `/`, so `/<tab>/evil.example` would lead to another site.
* Lengths are in characters (code points), as in the Rust validator.
*/
function isAllowedHref(href) {
	const length = [...href].length;
	if (length === 0 || length > LIMITS.maxHrefLength) return false;
	if (/[\u0000-\u0020\u007f\\]/.test(href)) return false;
	if (href.startsWith("#")) return true;
	if (href.startsWith("/")) return !href.startsWith("//");
	let url;
	try {
		url = new URL(href);
	} catch {
		return false;
	}
	return url.protocol === "https:" || url.protocol === "http:" || url.protocol === "mailto:";
}
/** Alt text rules: not only whitespace, at most 300 characters (code points). */
function isValidAlt(alt) {
	return alt.trim() !== "" && [...alt].length <= LIMITS.maxAltLength;
}
// ../../../packages/content-renderer/src/render.ts
var UnrenderableContent = class extends Error {
	name = "UnrenderableContent";
};
function escapeText(text) {
	return text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
}
function escapeAttribute(text) {
	return escapeText(text).replaceAll("\"", "&quot;").replaceAll("'", "&#39;");
}
function refuse(what) {
	throw new UnrenderableContent(what);
}
/** The list of child nodes, refusing anything that isn't a list of objects. */
function children(nodes) {
	if (nodes === void 0) return [];
	if (!Array.isArray(nodes)) refuse("content that is not a list");
	for (const node of nodes) if (typeof node !== "object" || node === null || typeof node.type !== "string") refuse("a node that is not an object with a type");
	return nodes;
}
function stringAttr(node, name) {
	const value = node.attrs?.[name];
	return typeof value === "string" ? value : refuse(`${node.type}.${name} is missing`);
}
/** Opening and closing tags for one mark. Links outside the site get rel=noopener noreferrer. */
function markTags(mark) {
	if (typeof mark !== "object" || mark === null) refuse("a mark that is not an object");
	switch (mark.type) {
		case "bold": return ["<strong>", "</strong>"];
		case "italic": return ["<em>", "</em>"];
		case "strike": return ["<s>", "</s>"];
		case "code": return ["<code>", "</code>"];
		case "link": {
			const href = mark.attrs?.["href"];
			if (typeof href !== "string" || !isAllowedHref(href)) refuse("a link that is not allowed");
			const external = /^https?:/i.test(href);
			return [`<a href="${escapeAttribute(href)}"${external ? " rel=\"noopener noreferrer\"" : ""}>`, "</a>"];
		}
		default: return refuse(`mark "${mark.type}"`);
	}
}
function inline(nodes) {
	let out = "";
	for (const node of children(nodes)) if (node.type === "hardBreak") out += "<br>";
	else if (node.type === "text") {
		if (typeof node.text !== "string" || node.text === "") refuse("an empty text node");
		let html = escapeText(node.text);
		if (node.marks !== void 0 && !Array.isArray(node.marks)) refuse("marks that are not a list");
		for (const mark of [...node.marks ?? []].reverse()) {
			const [open, close] = markTags(mark);
			html = `${open}${html}${close}`;
		}
		out += html;
	} else refuse(`"${node.type}" inside a paragraph`);
	return out;
}
function blocks(nodes, options) {
	return children(nodes).map((node) => block(node, options)).join("\n");
}
function block(node, options) {
	switch (node.type) {
		case "paragraph": return `<p>${inline(node.content)}</p>`;
		case "heading": {
			const level = node.attrs?.["level"];
			if (!HEADING_LEVELS.includes(level)) refuse(`heading level ${String(level)}`);
			return `<h${String(level)}>${inline(node.content)}</h${String(level)}>`;
		}
		case "bulletList": return `<ul>\n${blocks(node.content, options)}\n</ul>`;
		case "orderedList": {
			const start = node.attrs?.["start"];
			if (start !== void 0 && (!Number.isInteger(start) || start < 2)) refuse("an ordered list start");
			return `<ol${start === void 0 ? "" : ` start="${String(start)}"`}>\n${blocks(node.content, options)}\n</ol>`;
		}
		case "listItem": return `<li>${blocks(node.content, options)}</li>`;
		case "blockquote": return `<blockquote>\n${blocks(node.content, options)}\n</blockquote>`;
		case "codeBlock": {
			const language = node.attrs?.["language"];
			if (language !== void 0 && (typeof language !== "string" || !CODE_LANGUAGE.test(language))) refuse("a code block language");
			const text = children(node.content).map((child) => child.type === "text" && !child.marks ? child.text ?? "" : refuse("marks in a code block")).join("");
			return `<pre><code${language === void 0 ? "" : ` class="language-${language}"`}>${escapeText(text)}</code></pre>`;
		}
		case "horizontalRule": return "<hr>";
		case "image": {
			const assetId = stringAttr(node, "assetId");
			const alt = stringAttr(node, "alt");
			if (!ASSET_ID.test(assetId) || !isValidAlt(alt)) refuse("an image");
			return `<figure><img src="${escapeAttribute(options.assetUrl(assetId))}" alt="${escapeAttribute(alt)}" loading="lazy" decoding="async"></figure>`;
		}
		case "callout": {
			const tone = stringAttr(node, "tone");
			if (!CALLOUT_TONES.includes(tone)) refuse(`callout tone "${tone}"`);
			return `<aside class="callout callout-${tone}" role="note">\n${blocks(node.content, options)}\n</aside>`;
		}
		default: return refuse(`"${node.type}" as a block`);
	}
}
/** Renders a stored document. Throws `UnrenderableContent` for anything outside the format. */
function renderToHtml(value, options) {
	if (typeof value !== "object" || value === null) refuse("a value that is not an object");
	if (value.schema !== "colophon/rich-text" || value.version !== 1) refuse(`format ${String(value.schema)} v${String(value.version)}`);
	const doc = value.doc;
	if (typeof doc !== "object" || doc === null || doc.type !== "doc") refuse("no document");
	const content = children(doc.content);
	if (content.length === 0) refuse("a document without blocks");
	return blocks(content, options);
}
// ../shared/src/richtext.ts
/** The HTML of a `colophon/rich-text` v1 field, or `''` when the content has none. */
function richText(site, value) {
	if (value === void 0 || value === null) return "";
	return renderToHtml(value, { assetUrl: (assetId) => mediaUrl(site, assetId) });
}
// src/pages/members/posts/[id].astro
var _id__exports = /* @__PURE__ */ __exportAll({
	default: () => $$Id,
	file: () => $$file,
	prerender: () => false,
	url: () => $$url
});
createAstro("https://astro.build");
var $$Id = createComponent(async ($$result, $$props, $$slots) => {
	const Astro = $$result.createAstro($$props, $$slots);
	Astro.self = $$Id;
	const cms = site();
	const token = cms === null ? null : sessionToken(Astro.request);
	if (cms !== null && token === null) return Astro.redirect("/signin/");
	const user = cms !== null && token !== null ? await me(cms, token) : null;
	if (cms === null || user === null) return Astro.redirect("/signin/");
	const id = Astro.params.id ?? "";
	const post = await entry(cms, "posts", id, token ?? void 0);
	if (post === null) return Astro.redirect("/members/");
	const body = richText(cms, post.content["body"]);
	const tags = Array.isArray(post.content["tags"]) ? post.content["tags"].filter((tag) => typeof tag === "string") : [];
	return renderTemplate`${renderComponent($$result, "Base", $$Base, { "title": `${post.title} · The Draft` }, { "default": ($$result) => renderTemplate`${maybeRenderHead($$result)}<article class="post"><p class="gate-banner">Members only — this post is published as <code>${post.visibility}</code>; your session reads it as${" "}<code>role:${user.role}</code>.</p><span class="meta">${typeof post.content["published_on"] === "string" && post.content["published_on"]}${typeof post.content["author"] === "string" && ` · ${post.content["author"]}`}</span><h1>${post.title}</h1>${typeof post.content["excerpt"] === "string" && renderTemplate`<p class="lede">${post.content["excerpt"]}</p>`}<div class="tags">${tags.map((tag) => renderTemplate`<span class="tag">${tag}</span>`)}</div><div class="prose">${unescapeHTML(body)}</div><p><a href="/members/">← Back to the members area</a></p></article>` })}`;
}, "/Users/vinaysandhu/Desktop/Learning/akira-shirase-oss/projects/colophon/apps/sites/blog/src/pages/members/posts/[id].astro", void 0);
var $$file = "/Users/vinaysandhu/Desktop/Learning/akira-shirase-oss/projects/colophon/apps/sites/blog/src/pages/members/posts/[id].astro";
var $$url = "/members/posts/[id]";
// \0virtual:astro:page:src/pages/members/posts/[id]@_@astro
var page = () => _id__exports;
export { page };

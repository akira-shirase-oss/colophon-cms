import { t as __exportAll } from "./rolldown-runtime_782esTHO.mjs";
import { b as createAstro, g as addAttribute, l as renderTemplate, r as renderComponent, u as maybeRenderHead, x as createComponent } from "./server_BrZnmBTE.mjs";
import "./compiler_By5Sh0rm.mjs";
import { a as site, r as sessionToken } from "./session_Br3_L6V2.mjs";
import { t as entries } from "./delivery_CYqFMhoW.mjs";
import { t as me } from "./audience_zyCBDry1.mjs";
import { t as $$Base } from "./Base_BTcjQ9f7.mjs";
import { t as $$Unseeded } from "./Unseeded_Cpek91f7.mjs";
// src/pages/members/index.astro
var members_exports = /* @__PURE__ */ __exportAll({
	default: () => $$Index,
	file: () => $$file,
	prerender: () => false,
	url: () => $$url
});
createAstro("https://astro.build");
var $$Index = createComponent(async ($$result, $$props, $$slots) => {
	const Astro = $$result.createAstro($$props, $$slots);
	Astro.self = $$Index;
	const cms = site();
	const token = cms === null ? null : sessionToken(Astro.request);
	if (cms !== null && token === null) return Astro.redirect("/signin/");
	const user = cms !== null && token !== null ? await me(cms, token) : null;
	if (cms !== null && user === null) return Astro.redirect("/signin/");
	const posts = cms !== null && token !== null ? await entries(cms, "posts", token) : {
		items: [],
		generation: 0
	};
	const gated = posts.items.filter((post) => post.visibility !== "public");
	const open = posts.items.length;
	return renderTemplate`${renderComponent($$result, "Base", $$Base, {
		"title": "Members · The Draft",
		"generation": posts.generation
	}, { "default": ($$result) => renderTemplate`${cms === null ? renderTemplate`${renderComponent($$result, "Unseeded", $$Unseeded, {})}` : renderTemplate`${maybeRenderHead($$result)}<section class="account"><h1>Members</h1><p class="account-line">Signed in as <strong>${user?.display_name ?? user?.email}</strong> — role <code>${user?.role}</code>. Your session reads delivery as <code>role:${user?.role}</code>, so this list shows the ${gated.length}${" "}gated ${gated.length === 1 ? "post" : "posts"} anonymous readers can’t see (they get${" "}${open - gated.length}).</p><form method="post" action="/api/signout" class="signout"><button type="submit">Sign out</button></form><ul class="posts">${gated.map((post) => renderTemplate`<li><a${addAttribute(`/members/posts/${post.id}/`, "href")}><span class="meta">${typeof post.content["published_on"] === "string" && post.content["published_on"]}${typeof post.content["author"] === "string" && ` · ${post.content["author"]}`}</span><h2>${post.title}<span${addAttribute(`gate ${post.visibility === "authenticated" ? "gate-auth" : "gate-role"}`, "class")}>${post.visibility}</span></h2>${typeof post.content["excerpt"] === "string" && renderTemplate`<p class="excerpt">${post.content["excerpt"]}</p>`}</a></li>`)}</ul>${gated.length === 0 && renderTemplate`<p>Nothing gated is published yet — check back after the next release.</p>`}</section>`}` })}`;
}, "/Users/vinaysandhu/Desktop/Learning/akira-shirase-oss/projects/colophon/apps/sites/blog/src/pages/members/index.astro", void 0);
var $$file = "/Users/vinaysandhu/Desktop/Learning/akira-shirase-oss/projects/colophon/apps/sites/blog/src/pages/members/index.astro";
var $$url = "/members";
// \0virtual:astro:page:src/pages/members/index@_@astro
var page = () => members_exports;
export { page };

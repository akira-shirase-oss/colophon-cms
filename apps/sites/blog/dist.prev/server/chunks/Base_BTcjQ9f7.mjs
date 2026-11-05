import { b as createAstro, d as renderHead, g as addAttribute, l as renderTemplate, o as renderSlot, x as createComponent } from "./server_BrZnmBTE.mjs";
import "./compiler_By5Sh0rm.mjs";
// src/layouts/Base.astro
createAstro("https://astro.build");
var $$Base = createComponent(($$result, $$props, $$slots) => {
	const Astro = $$result.createAstro($$props, $$slots);
	Astro.self = $$Base;
	const { title, description = "The Draft, a demo blog published through Colophon", generation } = Astro.props;
	return renderTemplate`<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">${generation !== void 0 && generation > 0 && renderTemplate`<meta name="colophon-generation"${addAttribute(String(generation), "content")}>`}<title>${title}</title><meta name="description"${addAttribute(description, "content")}>${renderHead($$result)}</head><body><header class="site-header"><div class="wrap"><a class="brand" href="/">The Draft</a><nav class="site-nav"><a href="/">Latest posts</a><a href="/members/">Members</a></nav></div></header><main class="wrap">${renderSlot($$result, $$slots["default"])}</main><footer class="site-footer"><div class="wrap">${generation !== void 0 && generation > 0 ? renderTemplate`<p>Every word on this site was published through <a href="https://github.com/akira-shirase-oss/colophon-cms">Colophon</a> as release ${generation} of the demo-sites project, and this page names the release it was built from.</p>` : renderTemplate`<p>A Colophon demo site.</p>`}</div></footer></body></html>`;
}, "/Users/vinaysandhu/Desktop/Learning/akira-shirase-oss/projects/colophon/apps/sites/blog/src/layouts/Base.astro", void 0);
export { $$Base as t };

import { l as renderTemplate, u as maybeRenderHead, x as createComponent } from "./server_BrZnmBTE.mjs";
import "./compiler_By5Sh0rm.mjs";
// src/components/Unseeded.astro
var $$Unseeded = createComponent(($$result, $$props, $$slots) => {
	return renderTemplate`${maybeRenderHead($$result)}<section class="unseeded"><h1>The Draft is not seeded yet</h1><p>This demo blog reads the published <code>posts</code> collection of a Colophon project. Run the seed against a running server, then build again:</p><pre><code>scripts/seed-demo-sites.sh
npm run build -w @colophon/site-blog</code></pre><p>Or let <code>scripts/serve-demo-sites.sh</code> do both for all five sites.</p></section>`;
}, "/Users/vinaysandhu/Desktop/Learning/akira-shirase-oss/projects/colophon/apps/sites/blog/src/components/Unseeded.astro", void 0);
export { $$Unseeded as t };

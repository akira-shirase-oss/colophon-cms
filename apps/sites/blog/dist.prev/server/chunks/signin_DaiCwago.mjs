import { t as __exportAll } from "./rolldown-runtime_782esTHO.mjs";
import { b as createAstro, l as renderTemplate, r as renderComponent, u as maybeRenderHead, x as createComponent } from "./server_BrZnmBTE.mjs";
import "./compiler_By5Sh0rm.mjs";
import { a as site, r as sessionToken } from "./session_Br3_L6V2.mjs";
import { t as $$Base } from "./Base_BTcjQ9f7.mjs";
import { t as $$Unseeded } from "./Unseeded_Cpek91f7.mjs";
// src/pages/signin.astro
var signin_exports = /* @__PURE__ */ __exportAll({
	default: () => $$Signin,
	file: () => $$file,
	prerender: () => false,
	url: () => $$url
});
createAstro("https://astro.build");
var $$Signin = createComponent(($$result, $$props, $$slots) => {
	const Astro = $$result.createAstro($$props, $$slots);
	Astro.self = $$Signin;
	const cms = site();
	const token = sessionToken(Astro.request);
	if (cms !== null && token !== null) return Astro.redirect("/members/");
	const ERROR = {
		sign_in_failed: "That email and password don’t match an account.",
		too_many_attempts: "Too many tries — wait a few minutes and sign in again.",
		account_disabled: "This account has been disabled.",
		email_taken: "That email already has an account — sign in instead.",
		unavailable_role: "Signing up isn’t offered right now.",
		failed: "Something went wrong — try again."
	};
	const reason = Astro.url.searchParams.get("error") ?? "";
	const message = ERROR[reason] ?? (reason === "" ? null : ERROR.failed);
	return renderTemplate`${renderComponent($$result, "Base", $$Base, { "title": "Sign in · The Draft" }, { "default": ($$result) => renderTemplate`${cms === null ? renderTemplate`${renderComponent($$result, "Unseeded", $$Unseeded, {})}` : renderTemplate`${maybeRenderHead($$result)}<section class="account"><h1>Members</h1><p>Some posts on The Draft are for members only: they’re published with <code>role:member</code>${" "}visibility, and the delivery API answers them only for a live audience session that holds the<code>member</code> role. Sign in — or make an account — to read them. The seeded demo account is${" "}<code>member@draft.test</code> with password <code>member-of-the-draft</code>.</p>${message !== null && renderTemplate`<p class="form-error">${message}</p>`}<div class="account-forms"><form method="post" action="/api/signin" class="card"><h2>Sign in</h2><label>Email<input type="email" name="email" required autocomplete="email"></label><label>Password<input type="password" name="password" required autocomplete="current-password"></label><button type="submit">Sign in</button></form><form method="post" action="/api/signup" class="card"><h2>Sign up</h2><label>Email<input type="email" name="email" required autocomplete="email"></label><label>Password <small>(8+ characters)</small><input type="password" name="password" required minlength="8" autocomplete="new-password"></label><button type="submit">Create account</button></form></div></section>`}` })}`;
}, "/Users/vinaysandhu/Desktop/Learning/akira-shirase-oss/projects/colophon/apps/sites/blog/src/pages/signin.astro", void 0);
var $$file = "/Users/vinaysandhu/Desktop/Learning/akira-shirase-oss/projects/colophon/apps/sites/blog/src/pages/signin.astro";
var $$url = "/signin";
// \0virtual:astro:page:src/pages/signin@_@astro
var page = () => signin_exports;
export { page };

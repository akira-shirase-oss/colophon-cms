import { t as __exportAll } from "./rolldown-runtime_782esTHO.mjs";
import { a as site, i as setSession, n as redirect } from "./session_Br3_L6V2.mjs";
import { i as signUp } from "./audience_zyCBDry1.mjs";
// src/pages/api/signup.ts
var signup_exports = /* @__PURE__ */ __exportAll({
	POST: () => POST,
	prerender: () => false
});
var POST = async ({ request }) => {
	const cms = site();
	if (cms === null) return redirect("/");
	const form = await request.formData();
	const email = String(form.get("email") ?? "");
	const password = String(form.get("password") ?? "");
	const result = await signUp(cms, email, password);
	if ("error" in result) return redirect(`/signin/?error=${encodeURIComponent(result.error)}`);
	return redirect("/members/", (response) => setSession(response, result.ok.token));
};
// \0virtual:astro:page:src/pages/api/signup@_@ts
var page = () => signup_exports;
export { page };

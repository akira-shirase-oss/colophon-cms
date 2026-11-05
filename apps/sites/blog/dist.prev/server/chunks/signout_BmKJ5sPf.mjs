import { t as __exportAll } from "./rolldown-runtime_782esTHO.mjs";
import { a as site, n as redirect, r as sessionToken, t as clearSession } from "./session_Br3_L6V2.mjs";
import { r as signOut } from "./audience_zyCBDry1.mjs";
// src/pages/api/signout.ts
var signout_exports = /* @__PURE__ */ __exportAll({
	POST: () => POST,
	prerender: () => false
});
var POST = async ({ request }) => {
	const cms = site();
	const token = sessionToken(request);
	if (cms !== null && token !== null) await signOut(cms, token);
	return redirect("/", (response) => clearSession(response));
};
// \0virtual:astro:page:src/pages/api/signout@_@ts
var page = () => signout_exports;
export { page };

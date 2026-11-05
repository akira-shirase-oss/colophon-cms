import { readFileSync } from "node:fs";
import { resolve } from "node:path";
// ../shared/src/config.ts
/** The seed's record, `apps/sites/.colophon.json`, relative to the site's own directory
*  (`process.cwd()` while `astro build` runs). */
var RECORD = "../.colophon.json";
function fromRecord() {
	let parsed;
	try {
		parsed = JSON.parse(readFileSync(resolve(process.cwd(), RECORD), "utf8"));
	} catch {
		return null;
	}
	if (typeof parsed.api !== "string" || typeof parsed.project !== "string") return null;
	return {
		api: parsed.api,
		project: parsed.project,
		environment: typeof parsed.environment === "string" ? parsed.environment : "production"
	};
}
/** The site's connection, or `null` when nothing has been seeded yet. */
function site() {
	const env = process.env;
	if (env["COLOPHON_API"] !== void 0 && env["COLOPHON_PROJECT"] !== void 0) return {
		api: env["COLOPHON_API"],
		project: env["COLOPHON_PROJECT"],
		environment: env["COLOPHON_ENVIRONMENT"] ?? "production"
	};
	return fromRecord();
}
// src/lib/session.ts
/** The cookie the members pages and the api routes share. */
var SESSION_COOKIE = "draft_session";
/** Thirty days, like an audience session's own bound — the CMS still expires it first. */
var MAX_AGE = 2592e3;
function setSession(response, token) {
	response.headers.append("Set-Cookie", `${SESSION_COOKIE}=${token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=${MAX_AGE}`);
}
function clearSession(response) {
	response.headers.append("Set-Cookie", `${SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0`);
}
/** The token the request carries, if the browser sent one. */
function sessionToken(request) {
	const header = request.headers.get("cookie") ?? "";
	for (const part of header.split(";")) {
		const [name, ...rest] = part.trim().split("=");
		if (name === "draft_session") {
			const value = rest.join("=");
			return value === "" ? null : value;
		}
	}
	return null;
}
/** A 303 answer to a site path, with cookies applied. */
function redirect(path, apply) {
	const response = new Response(null, {
		status: 303,
		headers: { location: path }
	});
	apply?.(response);
	return response;
}
export { site as a, setSession as i, redirect as n, sessionToken as r, clearSession as t };

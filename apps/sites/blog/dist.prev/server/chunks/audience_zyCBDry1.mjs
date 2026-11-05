// ../shared/src/audience.ts
async function post(site, action, body) {
	const response = await fetch(`${site.api}/audience/v1/${site.project}/${action}`, {
		method: "POST",
		headers: {
			"content-type": "application/json",
			accept: "application/json"
		},
		body: JSON.stringify(body)
	});
	return {
		status: response.status,
		body: await response.json()
	};
}
function issued(body) {
	const made = body.data;
	if (made === void 0) return { error: body.error?.details?.reason ?? body.error?.code ?? "failed" };
	return { ok: {
		user: made.user,
		token: made.session,
		csrf: made.csrf
	} };
}
/** `POST /audience/v1/{project}/signup`: a new site user, in the project's open role. */
async function signUp(site, email, password, displayName) {
	const { body } = await post(site, "signup", {
		email,
		password,
		display_name: displayName
	});
	return issued(body);
}
/** `POST /audience/v1/{project}/signin`. */
async function signIn(site, email, password) {
	const { body } = await post(site, "signin", {
		email,
		password
	});
	return issued(body);
}
/** `POST /audience/v1/{project}/signout` as a bearer credential — best effort, never throws. */
async function signOut(site, token) {
	try {
		await fetch(`${site.api}/audience/v1/${site.project}/signout`, {
			method: "POST",
			headers: { authorization: `Bearer ${token}` }
		});
	} catch {}
}
/** `GET /audience/v1/{project}/me`: who the session belongs to, `null` when it is dead. */
async function me(site, token) {
	const response = await fetch(`${site.api}/audience/v1/${site.project}/me`, { headers: {
		authorization: `Bearer ${token}`,
		accept: "application/json"
	} });
	if (response.status === 401) return null;
	if (!response.ok) throw new Error(`GET /audience/v1/…/me: HTTP ${response.status}`);
	return (await response.json()).data ?? null;
}
export { signUp as i, signIn as n, signOut as r, me as t };

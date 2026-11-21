// Whether this SDK fits the server it talks to, and the plain words for when it doesn't (D-062:
// every client "says plainly when too old or too new"). The rule is the CLI's, ported from
// `crates/colophon-cli/src/compat.rs` with the same test table (docs/modules/08-cli.md,
// "Versions"). The server says what it is at `GET /version`: its release and the API versions it
// serves. This SDK fits when, in order:
//
// 1. the server serves the API version this SDK speaks (`API`);
// 2. before 1.0, both are on the same minor line (0.1.x with 0.1.x): until 1.0 a minor release
//    may change the API (SemVer item 4), so neither side can promise more;
// 3. the server is at least `OLDEST_SERVER`, the oldest that has everything this SDK calls.
//
// From 1.0 on, `/api/vN` is the contract: a newer server that still serves this API version
// fits. Pre-release tags (`-alpha.1`) are ordered by SemVer precedence and otherwise belong to
// their line.

/** This package's version — the workspace version every crate and npm package carries (module
 *  19, "Versions"); a test fails when it differs from `package.json` or `Cargo.toml`. */
export const SDK_VERSION = '0.1.0-alpha.3';

/** The API version this SDK speaks. */
export const API = 'v1';

/** The oldest server with everything this SDK calls. Raise it in the change that makes a method
 *  use a route or field older servers lack. 0.1.0-alpha.1 is the first server that reports its
 *  version. */
export const OLDEST_SERVER = '0.1.0-alpha.1';

const PACKAGE = '@colophon-cms/sdk';

/** A parsed SemVer 2.0 version. Build metadata is kept out: it never orders anything. */
export interface Version {
	readonly major: number;
	readonly minor: number;
	readonly patch: number;
	/** Pre-release identifiers: numbers where they are numeric, strings otherwise. */
	readonly pre: readonly (number | string)[];
	/** The version as written, build metadata dropped. */
	readonly text: string;
}

const NUMERIC = /^(0|[1-9]\d*)$/;
const IDENTIFIER = /^(0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)$/;
const BUILD = /^[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*$/;

/** `text` as a SemVer 2.0 version, or `null` when it isn't one (`1.0`, `v1.0.0`, `01.0.0`). */
export function parseVersion(text: string): Version | null {
	const [core, build, ...extra] = text.split('+');
	if (core === undefined || extra.length > 0 || (build !== undefined && !BUILD.test(build))) return null;
	const dash = core.indexOf('-');
	const release = dash === -1 ? core : core.slice(0, dash);
	const preText = dash === -1 ? null : core.slice(dash + 1);
	const parts = release.split('.');
	if (parts.length !== 3 || !parts.every((part) => NUMERIC.test(part))) return null;
	const [major, minor, patch] = parts.map(Number) as [number, number, number];
	if (![major, minor, patch].every(Number.isSafeInteger)) return null;
	const pre: (number | string)[] = [];
	if (preText !== null) {
		for (const identifier of preText.split('.')) {
			if (!IDENTIFIER.test(identifier)) return null;
			if (NUMERIC.test(identifier)) {
				const value = Number(identifier);
				if (!Number.isSafeInteger(value)) return null;
				pre.push(value);
			} else {
				pre.push(identifier);
			}
		}
	}
	return { major, minor, patch, pre, text: core };
}

/** SemVer 2.0 precedence (item 11): negative when `a` comes first, 0 when equal, positive after. */
export function comparePrecedence(a: Version, b: Version): number {
	for (const key of ['major', 'minor', 'patch'] as const) {
		if (a[key] !== b[key]) return a[key] < b[key] ? -1 : 1;
	}
	// A release follows its pre-releases.
	if (a.pre.length === 0 || b.pre.length === 0) return (a.pre.length === 0 ? 1 : 0) - (b.pre.length === 0 ? 1 : 0);
	for (let i = 0; i < Math.min(a.pre.length, b.pre.length); i++) {
		const x = a.pre[i]!;
		const y = b.pre[i]!;
		if (x === y) continue;
		if (typeof x === 'number' && typeof y === 'number') return x < y ? -1 : 1;
		// Numeric identifiers come before alphanumeric ones.
		if (typeof x === 'number') return -1;
		if (typeof y === 'number') return 1;
		return x < y ? -1 : 1;
	}
	return a.pre.length === b.pre.length ? 0 : a.pre.length < b.pre.length ? -1 : 1;
}

/** What a server's `/version` says, read loosely: extra fields are ignored. */
export interface ServerReport {
	readonly version: Version;
	readonly api: readonly string[];
}

/** The report in a `/version` body; `null` if it isn't one (a missing or malformed `version`, or
 *  an `api` that isn't a list of strings). */
export function readReport(body: unknown): ServerReport | null {
	if (typeof body !== 'object' || body === null) return null;
	const { version, api } = body as { version?: unknown; api?: unknown };
	if (typeof version !== 'string' || !Array.isArray(api) || !api.every((item) => typeof item === 'string')) return null;
	const parsed = parseVersion(version);
	return parsed === null ? null : { version: parsed, api: api as string[] };
}

/** Why a server doesn't fit. Each reason says which side should change. */
export type Misfit =
	/** The server serves only API versions newer than this SDK's: install a newer SDK. */
	| 'api_dropped'
	/** The server serves only API versions older than this SDK's: upgrade the server. */
	| 'api_not_yet'
	/** Before 1.0: the server is on a later minor line — install a newer SDK. */
	| 'later_line'
	/** Before 1.0: the server is on an earlier minor line — upgrade the server. */
	| 'earlier_line'
	/** The server is older than `OLDEST_SERVER` — upgrade the server. */
	| 'before_oldest'
	/** The server has no `/version` at all, so it predates `OLDEST_SERVER`. */
	| 'no_version_report';

/** True when this SDK is too old for the server (install a newer SDK); false when it is too new
 *  (upgrade the server, or install an older SDK). */
export function sdkTooOld(misfit: Misfit): boolean {
	return misfit === 'api_dropped' || misfit === 'later_line';
}

/** `v1` → 1. */
function apiNumber(api: string): number | null {
	const match = /^v(\d+)$/.exec(api);
	return match === null ? null : Number(match[1]);
}

/** The release line a version promises compatibility within: its major version, or before 1.0
 *  its minor one. */
function line(version: Version): [number, number] {
	return version.major === 0 ? [0, version.minor] : [version.major, 0];
}

function lineName(version: Version): string {
	const [major, minor] = line(version);
	return major === 0 ? `0.${minor}` : String(major);
}

/** The rule above, as a pure function: `null` when the server fits. */
export function verdict(client: Version, api: string, oldestServer: Version, server: ServerReport): Misfit | null {
	if (!server.api.includes(api)) {
		const offered = server.api.map(apiNumber).filter((n): n is number => n !== null);
		const ours = apiNumber(api);
		let newer: boolean;
		if (ours !== null && offered.length > 0 && offered.every((theirs) => theirs > ours)) newer = true;
		else if (ours !== null && offered.length > 0 && offered.every((theirs) => theirs < ours)) newer = false;
		// No clear order between the API versions: go by release.
		else newer = comparePrecedence(server.version, client) > 0;
		return newer ? 'api_dropped' : 'api_not_yet';
	}
	if (client.major === 0 || server.version.major === 0) {
		const [a, b] = line(server.version);
		const [c, d] = line(client);
		if (a !== c || b !== d) return a > c || (a === c && b > d) ? 'later_line' : 'earlier_line';
	}
	if (comparePrecedence(server.version, oldestServer) < 0) return 'before_oldest';
	return null;
}

/** The plain-words explanation for a misfit, naming both versions and what to do. */
export function explain(misfit: Misfit, client: Version, oldest: Version, serverUrl: string, report: ServerReport | null): string {
	if (misfit === 'no_version_report' || report === null) {
		return (
			`${PACKAGE} ${client.text} is too new for the server at ${serverUrl}: the server doesn't report its ` +
			`version, so it predates ${oldest.text}, the oldest server this SDK works with. Upgrade the server.`
		);
	}
	const server = report.version.text;
	const apis = report.api.length === 0 ? 'none' : report.api.join(', ');
	const preOne = (side: string) =>
		`before 1.0 each minor release may change the API, so the SDK and the server must share a minor ` +
		`version, and the server is on ${side} one (${lineName(client)} here, ${lineName(report.version)} there)`;
	const why: Record<Exclude<Misfit, 'no_version_report'>, string> = {
		api_dropped: `the server no longer serves API ${API}, which this SDK speaks (it serves ${apis})`,
		api_not_yet: `this SDK speaks API ${API}, which the server doesn't serve (it serves ${apis})`,
		later_line: preOne('a later'),
		earlier_line: preOne('an earlier'),
		before_oldest: `this SDK needs a server of at least ${oldest.text}`
	};
	return sdkTooOld(misfit)
		? `${PACKAGE} ${client.text} is too old for the server at ${serverUrl}, which runs ${server}: ${why[misfit]}. ` +
				`Install ${PACKAGE}@${server} to match it.`
		: `${PACKAGE} ${client.text} is too new for the server at ${serverUrl}, which runs ${server}: ${why[misfit]}. ` +
				`Upgrade the server, or install ${PACKAGE}@${server} to match it.`;
}

/** What `client.compatibility()` found. */
export interface Compatibility {
	/** Whether this SDK and the server fit each other. */
	readonly compatible: boolean;
	/** This SDK's version. */
	readonly sdk: string;
	/** The server's release, or `null` when it doesn't report one. */
	readonly server: string | null;
	/** The API versions the server serves. */
	readonly api: readonly string[];
	/** Why not, as a stable code; absent when compatible. */
	readonly misfit?: Misfit;
	/** Why not, in plain words — which side should move and to what; absent when compatible. */
	readonly reason?: string;
}

/** The SDK's own versions, parsed; a broken constant is a bug, so it throws. */
export function ours(): { client: Version; oldest: Version } {
	const client = parseVersion(SDK_VERSION);
	const oldest = parseVersion(OLDEST_SERVER);
	if (client === null || oldest === null) throw new Error('the SDK carries an invalid version constant');
	return { client, oldest };
}

/** The whole check for one `/version` answer: `report` is `null` when the route doesn't exist. */
export function judge(serverUrl: string, report: ServerReport | null): Compatibility {
	const { client, oldest } = ours();
	const misfit = report === null ? 'no_version_report' : verdict(client, API, oldest, report);
	const base = { sdk: client.text, server: report?.version.text ?? null, api: report?.api ?? [] };
	return misfit === null
		? { compatible: true, ...base }
		: { compatible: false, ...base, misfit, reason: explain(misfit, client, oldest, serverUrl, report) };
}

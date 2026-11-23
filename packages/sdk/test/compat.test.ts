// The server-version check (D-062): the CLI's rule from crates/colophon-cli/src/compat.rs, with
// the same table, then the client's lazy check — once, before the first API call, `warn` beside
// the call and `throw` in front of it.

/// <reference types="node" />
import { readFileSync } from 'node:fs';
import { describe, expect, it, vi } from 'vitest';
import { ApiError, Client, SDK_VERSION } from '../src/index.ts';
import {
	comparePrecedence,
	explain,
	OLDEST_SERVER,
	ours,
	parseVersion,
	readReport,
	sdkTooOld,
	verdict,
	type Misfit,
	type ServerReport,
	type Version
} from '../src/compat.ts';

function v(text: string): Version {
	const version = parseVersion(text);
	if (version === null) throw new Error(`not a version: ${text}`);
	return version;
}

function server(version: string, api: string[]): ServerReport {
	return { version: v(version), api };
}

/** The verdict for an SDK at `client` speaking v1 that needs at least 0.1.0-alpha.1. */
function judge(client: string, version: string, api: string[]): Misfit | null {
	return verdict(v(client), 'v1', v('0.1.0-alpha.1'), server(version, api));
}

describe('the rule, as compat.rs states it', () => {
	it('the same release fits', () => {
		expect(judge('0.1.0-alpha.1', '0.1.0-alpha.1', ['v1'])).toBeNull();
		expect(judge('1.4.2', '1.4.2', ['v1'])).toBeNull();
	});

	it('before 1.0 the minor line must match, in both directions', () => {
		// Pre-releases and patches within a line fit, whichever side is newer.
		expect(judge('0.1.0-alpha.1', '0.1.0-alpha.3', ['v1'])).toBeNull();
		expect(judge('0.1.0', '0.1.0-alpha.2', ['v1'])).toBeNull();
		expect(judge('0.1.0-alpha.1', '0.1.7', ['v1'])).toBeNull();
		expect(judge('0.1.7', '0.1.0', ['v1'])).toBeNull();
		// Another minor line doesn't, and the verdict says which side should move.
		expect(judge('0.1.0-alpha.1', '0.2.0', ['v1'])).toBe('later_line');
		expect(judge('0.3.1', '0.2.9', ['v1'])).toBe('earlier_line');
		// Crossing 1.0 is a line change too.
		expect(judge('0.9.0', '1.0.0', ['v1'])).toBe('later_line');
		expect(judge('1.0.0', '0.9.0', ['v1'])).toBe('earlier_line');
	});

	it('from 1.0 the API version is the contract', () => {
		expect(judge('1.0.0', '1.9.0', ['v1'])).toBeNull();
		expect(judge('1.9.0', '1.0.0', ['v1'])).toBeNull();
		expect(judge('1.2.0', '2.0.0', ['v1', 'v2'])).toBeNull();
		expect(judge('1.2.0', '3.0.0', ['v2'])).toBe('api_dropped');
		expect(verdict(v('2.0.0'), 'v2', v('2.0.0'), server('1.8.0', ['v1']))).toBe('api_not_yet');
	});

	it('a server older than the oldest supported is too old for this SDK', () => {
		const oldest = v('1.3.0');
		expect(verdict(v('1.5.0'), 'v1', oldest, server('1.2.9', ['v1']))).toBe('before_oldest');
		// A pre-release comes before its release.
		expect(verdict(v('1.5.0'), 'v1', oldest, server('1.3.0-rc.1', ['v1']))).toBe('before_oldest');
		expect(verdict(v('1.5.0'), 'v1', oldest, server('1.3.0', ['v1']))).toBeNull();
	});

	it('the API check comes first and handles odd lists', () => {
		// The API decides even within one line.
		expect(judge('0.1.0', '0.1.5', ['v2'])).toBe('api_dropped');
		expect(verdict(v('0.1.0'), 'v2', v('0.1.0'), server('0.1.5', ['v1']))).toBe('api_not_yet');
		// Nothing served, or names we can't order: the releases decide the direction.
		expect(judge('0.1.0', '0.1.5', [])).toBe('api_dropped');
		expect(judge('0.1.5', '0.1.0', ['beta'])).toBe('api_not_yet');
		// v0 and v2 around our v1: no clear order either.
		expect(judge('1.0.0', '2.0.0', ['v0', 'v2'])).toBe('api_dropped');
	});

	it('reports are read loosely but never guessed', () => {
		const body = { version: '0.1.0-alpha.1', commit: 'abc', build_date: '2026-10-28', api: ['v1'], added_later: true };
		expect(readReport(body)).toEqual(server('0.1.0-alpha.1', ['v1']));
		for (const broken of [
			{ api: ['v1'] },
			{ version: '1.0', api: ['v1'] },
			{ version: '0.1.0' },
			{ version: '0.1.0', api: 'v1' },
			{ version: '0.1.0', api: [1] },
			{ error: { code: 'route_not_found' } },
			null,
			'0.1.0'
		]) {
			expect(readReport(broken), JSON.stringify(broken)).toBeNull();
		}
	});

	it('each reason says which side should move', () => {
		expect(sdkTooOld('api_dropped')).toBe(true);
		expect(sdkTooOld('later_line')).toBe(true);
		expect(sdkTooOld('api_not_yet')).toBe(false);
		expect(sdkTooOld('earlier_line')).toBe(false);
		expect(sdkTooOld('before_oldest')).toBe(false);
		expect(sdkTooOld('no_version_report')).toBe(false);
	});

	it('explanations name both versions and what to do', () => {
		const url = 'https://cms.example';
		expect(explain('later_line', v('0.1.0'), v('0.1.0-alpha.1'), url, server('0.2.0', ['v1']))).toBe(
			'@colophon-cms/sdk 0.1.0 is too old for the server at https://cms.example, which runs 0.2.0: ' +
				'before 1.0 each minor release may change the API, so the SDK and the server must share a minor ' +
				'version, and the server is on a later one (0.1 here, 0.2 there). Install @colophon-cms/sdk@0.2.0 to match it.'
		);
		expect(explain('before_oldest', v('1.5.0'), v('1.3.0'), url, server('1.2.0', ['v1']))).toMatch(
			/^@colophon-cms\/sdk 1\.5\.0 is too new for the server at https:\/\/cms\.example, which runs 1\.2\.0: this SDK needs a server of at least 1\.3\.0\. Upgrade the server/
		);
		expect(explain('api_dropped', v('1.0.0'), v('1.0.0'), url, server('2.0.0', ['v2']))).toContain(
			'no longer serves API v1, which this SDK speaks (it serves v2)'
		);
		expect(explain('no_version_report', v('0.1.0'), v('0.1.0-alpha.1'), url, null)).toContain(
			"the server doesn't report its version, so it predates 0.1.0-alpha.1"
		);
	});
});

describe('SemVer, as the semver crate reads it', () => {
	it('orders by precedence, pre-releases before their release', () => {
		const ordered = [
			'1.0.0-alpha',
			'1.0.0-alpha.1',
			'1.0.0-alpha.beta',
			'1.0.0-beta',
			'1.0.0-beta.2',
			'1.0.0-beta.11',
			'1.0.0-rc.1',
			'1.0.0',
			'1.0.1',
			'1.1.0',
			'2.0.0'
		];
		for (let i = 0; i + 1 < ordered.length; i++) {
			expect(comparePrecedence(v(ordered[i]!), v(ordered[i + 1]!)), `${ordered[i]} < ${ordered[i + 1]}`).toBeLessThan(0);
			expect(comparePrecedence(v(ordered[i + 1]!), v(ordered[i]!))).toBeGreaterThan(0);
		}
		expect(comparePrecedence(v('1.0.0+build.1'), v('1.0.0+build.2'))).toBe(0);
	});

	it('refuses what is not a version', () => {
		for (const text of ['1.0', 'v1.0.0', '01.0.0', '1.0.0-', '1.0.0-01', '1.0.0+', '1.0.0-a..b', '', '1.0.0 ']) {
			expect(parseVersion(text), text).toBeNull();
		}
	});
});

describe('this package carries the one version (module 19, "Versions")', () => {
	it('SDK_VERSION is package.json and Cargo.toml', () => {
		const pkg = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')) as { version: string };
		const cargo = readFileSync(new URL('../../../Cargo.toml', import.meta.url), 'utf8');
		const workspace = /\[workspace\.package\][^[]*?\nversion\s*=\s*"([^"]+)"/.exec(cargo);
		expect(workspace?.[1]).toBe(SDK_VERSION);
		expect(pkg.version).toBe(SDK_VERSION);
		const { client, oldest } = ours();
		expect(comparePrecedence(oldest, client), `OLDEST_SERVER (${OLDEST_SERVER}) is newer than the SDK`).toBeLessThanOrEqual(0);
	});
});

/* --------------------------------------------------------- the client's check */

function versionAnswer(version: string, api = ['v1']): Response {
	return new Response(JSON.stringify({ version, commit: 'abc', build_date: '2026-11-17', api }), {
		status: 200,
		headers: { 'content-type': 'application/json' }
	});
}

function ok(): Response {
	return new Response(JSON.stringify({ data: [], next_cursor: null }), { status: 200, headers: { 'content-type': 'application/json' } });
}

/** A fetch answering `/version` with `version` (a status number: that failure; an Error: no
 *  answer) and every other path with an empty page; it records the paths asked for. */
function stubServer(version: string | number | Error) {
	const paths: string[] = [];
	const impl = (async (input: RequestInfo | URL) => {
		const path = new URL(String(input)).pathname;
		paths.push(path);
		if (path === '/version') {
			if (version instanceof Error) throw version;
			if (typeof version === 'number') {
				return new Response(JSON.stringify({ error: { code: 'route_not_found', message: 'no route', request_id: 'r' } }), {
					status: version,
					headers: { 'content-type': 'application/json' }
				});
			}
			return versionAnswer(version);
		}
		return ok();
	}) as typeof globalThis.fetch;
	return { paths, fetch: impl };
}

describe('client.compatibility()', () => {
	it('answers sdk, server and api, and the reason when they do not fit', async () => {
		const fits = await new Client({ baseUrl: 'http://cms.test', fetch: stubServer(SDK_VERSION).fetch, compat: 'off' }).compatibility();
		expect(fits).toEqual({ compatible: true, sdk: SDK_VERSION, server: SDK_VERSION, api: ['v1'] });

		const later = await new Client({ baseUrl: 'http://cms.test', fetch: stubServer('0.2.0').fetch, compat: 'off' }).compatibility();
		expect(later.compatible).toBe(false);
		expect(later.misfit).toBe('later_line');
		expect(later.reason).toContain('is too old for the server at http://cms.test, which runs 0.2.0');
	});

	it('a server with no /version predates every supported one', async () => {
		const result = await new Client({ baseUrl: 'http://cms.test', fetch: stubServer(404).fetch, compat: 'off' }).compatibility();
		expect(result).toMatchObject({ compatible: false, server: null, api: [], misfit: 'no_version_report' });
	});

	it('an answer that is not a report is an invalid_response, never a guess', async () => {
		const fetch = (async () => new Response('{"version": 3}', { status: 200 })) as typeof globalThis.fetch;
		const error = await new Client({ baseUrl: 'http://cms.test', fetch, compat: 'off' }).compatibility().catch((e: unknown) => e);
		expect(error).toBeInstanceOf(ApiError);
		expect((error as ApiError).code).toBe('invalid_response');
	});
});

describe("compat: 'throw'", () => {
	it('checks once, before the first call, and lets every call through when the server fits', async () => {
		const { paths, fetch } = stubServer(SDK_VERSION);
		const env = new Client({ baseUrl: 'http://cms.test', fetch, compat: 'throw' }).environment('p');
		await env.listEntries('articles');
		await env.listEntries('articles');
		expect(paths.filter((p) => p === '/version')).toHaveLength(1);
		expect(paths[0]).toBe('/version');
		expect(paths).toHaveLength(3);
	});

	it('fails every call, sending nothing, while the server does not fit', async () => {
		const { paths, fetch } = stubServer('0.2.0');
		const env = new Client({ baseUrl: 'http://cms.test', fetch, compat: 'throw' }).environment('p');
		for (let i = 0; i < 2; i++) {
			const error = await env.listEntries('articles').catch((e: unknown) => e);
			expect(error).toBeInstanceOf(ApiError);
			expect((error as ApiError).code).toBe('incompatible_server');
			expect((error as ApiError).retryable).toBe(false);
			expect((error as ApiError).message).toContain('Install @colophon-cms/sdk@0.2.0');
		}
		expect(paths).toEqual(['/version']);
	});

	it('a check with no answer decides nothing: the call goes, and the next call asks again', async () => {
		const { paths, fetch } = stubServer(new TypeError('connection refused'));
		const env = new Client({ baseUrl: 'http://cms.test', fetch, compat: 'throw' }).environment('p');
		await env.listEntries('articles');
		await env.listEntries('articles');
		expect(paths).toEqual(['/version', '/api/v1/projects/p/environments/production/collections/articles/entries', '/version', '/api/v1/projects/p/environments/production/collections/articles/entries']);
	});

	it('health and version never wait on the check', async () => {
		const { paths, fetch } = stubServer('0.2.0');
		const client = new Client({ baseUrl: 'http://cms.test', fetch, compat: 'throw' });
		await client.livez();
		await client.version();
		expect(paths).toEqual(['/livez', '/version']);
	});
});

describe("compat: 'warn', the default", () => {
	it('says why once, beside the calls, and never fails them', async () => {
		const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
		try {
			const { paths, fetch } = stubServer('0.2.0');
			const env = new Client({ baseUrl: 'http://cms.test', fetch }).environment('p');
			await env.listEntries('articles');
			await env.listEntries('articles');
			await new Promise((resolve) => setTimeout(resolve, 0));
			await env.listEntries('articles');
			expect(paths.filter((p) => p === '/version')).toHaveLength(1);
			expect(warn).toHaveBeenCalledTimes(1);
			expect(String(warn.mock.calls[0]![0])).toContain(`@colophon-cms/sdk ${SDK_VERSION} is too old`);
		} finally {
			warn.mockRestore();
		}
	});

	it("'off' never asks", async () => {
		const { paths, fetch } = stubServer('0.2.0');
		await new Client({ baseUrl: 'http://cms.test', fetch, compat: 'off' }).environment('p').listEntries('articles');
		expect(paths).not.toContain('/version');
	});

	it('refuses a mode it does not know', () => {
		expect(() => new Client({ baseUrl: 'http://cms.test', compat: 'loud' as 'warn' })).toThrow(TypeError);
	});
});

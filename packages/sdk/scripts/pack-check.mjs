#!/usr/bin/env node
// The packaging gate for the npm packages people install to build on Colophon (D-170):
// @colophon-cms/sdk and @colophon-cms/rich-text. It checks what npm would actually ship, not the
// source tree:
//
//   1. `npm pack` each package (prepack builds it) and hold the tarball to an allowlist — dist/
//      (.js and .d.ts only), README.md, LICENSE, CHANGELOG.md, package.json — and a size budget;
//      the packed package.json must carry the workspace version and no runtime dependencies (so
//      never a private `@colophon/*` name: dependency confusion).
//   2. publint --strict and Are the Types Wrong (`--profile esm-only`) on each tarball.
//   3. In a scratch project with only the two tarballs installed: a Node ESM script that calls
//      the SDK against a stub server (livez, the compatibility check, a delivery read) and
//      renders every rich-text fixture to its reviewed HTML; a TypeScript type-check of a
//      consumer file and of every `ts` code block in both READMEs, under `nodenext` and
//      `bundler` resolution with each TypeScript the workspace uses; and a browser bundle of
//      `Client` and `Delivery`, minified and gzipped, held to a budget.
//
//   node packages/sdk/scripts/pack-check.mjs              # tarballs in a temporary directory
//   node packages/sdk/scripts/pack-check.mjs --out dist   # keeps them there (the release job)
//
// PACK_CHECK_PORT picks the stub server's port (default: any free one).
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

const here = path.dirname(fileURLToPath(import.meta.url));
const repository = path.resolve(here, '../../..');
const require = createRequire(import.meta.url);

/** Packed-size budgets (gzipped tarball bytes) and the browser bundle's (min+gzip). */
const PACKED_BUDGET = { '@colophon-cms/sdk': 150 * 1024, '@colophon-cms/rich-text': 20 * 1024 };
const BROWSER_BUDGET = 12 * 1024;
/** Every file a tarball may hold besides dist/. */
const ALLOWED = new Set(['package.json', 'README.md', 'LICENSE', 'CHANGELOG.md']);
const PACKAGES = [
	{ name: '@colophon-cms/sdk', dir: 'packages/sdk' },
	{ name: '@colophon-cms/rich-text', dir: 'packages/rich-text' }
];

const failures = [];
function check(ok, message) {
	if (ok) return;
	failures.push(message);
	console.error(`  FAIL ${message}`);
}
function step(title) {
	console.log(`pack-check: ${title}`);
}
function run(command, args, options = {}) {
	return execFileSync(command, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], ...options });
}
function runShown(command, args, options = {}) {
	try {
		execFileSync(command, args, { stdio: 'inherit', ...options });
		return true;
	} catch {
		return false;
	}
}
const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';

/** The release version: `[workspace.package] version` in Cargo.toml (module 19, "Versions"). */
function workspaceVersion() {
	const cargo = readFileSync(path.join(repository, 'Cargo.toml'), 'utf8');
	const match = /\[workspace\.package\][^[]*?\nversion\s*=\s*"([^"]+)"/.exec(cargo);
	if (match === null) throw new Error('Cargo.toml has no [workspace.package] version');
	return match[1];
}

const argv = process.argv.slice(2);
const outIndex = argv.indexOf('--out');
const keep = outIndex !== -1;
const scratch = mkdtempSync(path.join(tmpdir(), 'colophon-pack-check-'));
const out = keep ? path.resolve(argv[outIndex + 1] ?? '') : path.join(scratch, 'tarballs');
mkdirSync(out, { recursive: true });
const version = workspaceVersion();

/* ------------------------------------------------------------------ 1. pack */

const tarballs = {};
for (const { name, dir } of PACKAGES) {
	step(`packing ${name}`);
	const [packed] = JSON.parse(run(npm, ['pack', '--json', '--pack-destination', out], { cwd: path.join(repository, dir) }));
	const tarball = path.join(out, packed.filename);
	tarballs[name] = tarball;
	const files = packed.files.map((file) => file.path).sort();
	check(packed.name === name, `${dir} packs as ${packed.name}, not ${name}`);
	check(packed.version === version, `${name} is ${packed.version}, but the workspace is ${version} (module 19, "Versions")`);
	for (const file of files) {
		const allowed = ALLOWED.has(file) || (file.startsWith('dist/') && (file.endsWith('.js') || file.endsWith('.d.ts')));
		check(allowed, `${name} would ship ${file}, which is not on the allowlist`);
	}
	for (const needed of ['package.json', 'README.md', 'LICENSE', 'CHANGELOG.md', 'dist/index.js', 'dist/index.d.ts']) {
		check(files.includes(needed), `${name} would ship without ${needed}`);
	}
	check(packed.size <= PACKED_BUDGET[name], `${name} packs to ${packed.size} bytes, over its ${PACKED_BUDGET[name]}-byte budget`);
	const manifest = JSON.parse(run('tar', ['-xOzf', tarball, 'package/package.json']));
	for (const field of ['dependencies', 'peerDependencies', 'optionalDependencies', 'bundleDependencies', 'bundledDependencies']) {
		const names = Object.keys(manifest[field] ?? {});
		check(names.length === 0, `${name} names ${field} (${names.join(', ')}): it ships with none`);
		check(!names.some((dep) => dep.startsWith('@colophon/')), `${name} depends on a private @colophon/* name`);
	}
	check(manifest.type === 'module' && manifest.exports?.['.'] !== undefined, `${name} is not the ESM-only package it promises`);
	console.log(`  ${packed.filename}: ${files.length} files, ${packed.size} bytes packed, ${packed.unpackedSize} unpacked`);
}

/* --------------------------------------------------------- 2. publint, attw */

const bin = (name) => path.join(repository, 'node_modules', '.bin', name);
for (const { name } of PACKAGES) {
	step(`publint --strict ${name}`);
	check(runShown(bin('publint'), ['run', tarballs[name], '--strict']), `publint refused ${name}`);
	step(`attw --profile esm-only ${name}`);
	check(runShown(bin('attw'), [tarballs[name], '--profile', 'esm-only', '--format', 'table']), `Are the Types Wrong refused ${name}`);
}

/* -------------------------------------------------------- 3. a consumer project */

step('installing both tarballs in a scratch project');
const consumer = path.join(scratch, 'consumer');
mkdirSync(consumer, { recursive: true });
writeFileSync(path.join(consumer, 'package.json'), JSON.stringify({ name: 'consumer', private: true, type: 'module' }, null, 2));
run(npm, ['install', '--no-audit', '--no-fund', '--ignore-scripts', '--no-package-lock', tarballs['@colophon-cms/sdk'], tarballs['@colophon-cms/rich-text']], {
	cwd: consumer
});

// 3a. Node ESM against a stub server.
step('Node ESM: the SDK against a stub server, the renderer against every fixture');
const fixtures = path.join(repository, 'fixtures/rich-text/v1');
const sampleBody = JSON.parse(readFileSync(path.join(fixtures, 'valid', readdirSync(path.join(fixtures, 'valid')).sort()[1]), 'utf8')).value;
const server = createServer((request, response) => {
	const url = new URL(request.url ?? '/', 'http://stub');
	const json = (status, body) => {
		response.writeHead(status, { 'content-type': 'application/json', 'x-request-id': '0190a4b0-0000-7000-8000-000000000001' });
		response.end(JSON.stringify(body));
	};
	if (url.pathname === '/livez') return json(200, { status: 'live' });
	if (url.pathname === '/version') return json(200, { version, commit: 'stub', build_date: '2026-11-17', api: ['v1'] });
	if (url.pathname === '/content/v1/site/production/articles') {
		return json(200, { data: [{ id: '0190a4b0-0000-7000-8000-0000000000aa', title: 'Hello', content: { body: sampleBody } }], next_cursor: null });
	}
	return json(404, { error: { code: 'route_not_found', message: 'no such route', request_id: 'stub' } });
});
await new Promise((resolve) => server.listen(Number(process.env.PACK_CHECK_PORT ?? 0), '127.0.0.1', resolve));
const origin = `http://127.0.0.1:${server.address().port}`;
writeFileSync(
	path.join(consumer, 'smoke.mjs'),
	`import { readdirSync, readFileSync } from 'node:fs';
import { Client, SDK_VERSION } from '@colophon-cms/sdk';
import { renderToHtml, UnrenderableContent } from '@colophon-cms/rich-text';
import sdkPackage from '@colophon-cms/sdk/package.json' with { type: 'json' };
import richTextPackage from '@colophon-cms/rich-text/package.json' with { type: 'json' };

const [origin, fixtures] = process.argv.slice(2);
const client = new Client({ baseUrl: origin, compat: 'throw' });
const live = await client.livez();
const compatibility = await client.compatibility();
const page = await client.delivery('site').list('articles', { limit: 1 });
const assetUrl = (id) => \`\${origin}/media/v1/site/production/\${id}\`;
const html = renderToHtml(page.data[0].content.body, { assetUrl });
let rendered = 0;
const mismatched = [];
for (const file of readdirSync(\`\${fixtures}/valid\`).filter((f) => f.endsWith('.json')).sort()) {
	const { value } = JSON.parse(readFileSync(\`\${fixtures}/valid/\${file}\`, 'utf8'));
	const reviewed = readFileSync(\`\${fixtures}/rendered/\${file.replace(/\\.json$/, '.html')}\`, 'utf8');
	if (renderToHtml(value, { assetUrl: (id) => \`https://assets.example/\${id}\` }) + '\\n' !== reviewed) mismatched.push(file);
	rendered++;
}
let refused = false;
try {
	renderToHtml({ schema: 'colophon/rich-text', version: 99, doc: { type: 'doc' } }, { assetUrl });
} catch (error) {
	refused = error instanceof UnrenderableContent;
}
console.log(JSON.stringify({
	live: live.response.status,
	compatible: compatibility.compatible,
	sdk: SDK_VERSION,
	packageVersions: [sdkPackage.version, richTextPackage.version],
	html: html.length > 0,
	rendered,
	mismatched,
	refused
}));
`
);
let smoke = null;
try {
	const output = await new Promise((resolve, reject) => {
		import('node:child_process').then(({ execFile }) =>
			execFile(process.execPath, ['smoke.mjs', origin, fixtures], { cwd: consumer, encoding: 'utf8' }, (error, stdout, stderr) =>
				error ? reject(new Error(`${error.message}\n${stderr}`)) : resolve(stdout)
			)
		);
	});
	smoke = JSON.parse(output.trim().split('\n').pop());
} catch (error) {
	check(false, `the Node ESM smoke script failed: ${error.message}`);
} finally {
	server.close();
}
if (smoke !== null) {
	console.log(`  ${JSON.stringify(smoke)}`);
	check(smoke.live === 200, 'livez through the packed SDK did not answer 200');
	check(smoke.compatible === true, 'the packed SDK judged its own release incompatible');
	check(smoke.sdk === version && smoke.packageVersions.every((v) => v === version), 'SDK_VERSION or a packed package.json is not the release version');
	check(smoke.html, 'a delivered body did not render');
	check(smoke.rendered > 5 && smoke.mismatched.length === 0, `the packed renderer differs from the reviewed HTML for ${smoke.mismatched.join(', ')}`);
	check(smoke.refused, 'the packed renderer did not refuse an unknown format version');
}

// 3b. TypeScript, as consumers compile: a consumer file and every README `ts` block.
step('TypeScript: a consumer file and the READMEs, under nodenext and bundler');
writeFileSync(
	path.join(consumer, 'consumer.ts'),
	`import { ApiError, Client, Delivery, SDK_VERSION, type Compatibility, type PublishedEntryView } from '@colophon-cms/sdk';
import type { paths } from '@colophon-cms/sdk/api.types';
import { renderToHtml, RENDERER_VERSION, type RichTextValue } from '@colophon-cms/rich-text';

const client = new Client({ baseUrl: 'http://localhost:58000', compat: 'warn' });
const delivery: Delivery = client.delivery('site').atGeneration(3);
const page = await delivery.list('articles', { limit: 10 });
const first: PublishedEntryView | undefined = page.data[0];
const body = first?.content?.['body'] as RichTextValue | undefined;
export const html: string = body === undefined ? '' : renderToHtml(body, { assetUrl: (id: string) => id });
export const check: Compatibility = await client.compatibility();
export const versions: [string, number] = [SDK_VERSION, RENDERER_VERSION];
export type Delivered = paths['/content/v1/{project}/{environment}/{collection}']['get'];
export const failed = (error: unknown): string | null => (error instanceof ApiError ? error.code : null);
`
);
const readmes = [
	{ name: 'sdk', file: path.join(repository, 'packages/sdk/README.md') },
	{ name: 'rich-text', file: path.join(repository, 'packages/rich-text/README.md') }
];
// The names README examples leave to the reader, declared once for every block.
const prelude = 'declare const token: string;\ndeclare const projectId: string;\ndeclare const entryId: string;\n';
let blocks = 0;
for (const { name, file } of readmes) {
	const text = readFileSync(file, 'utf8');
	for (const [, code] of text.matchAll(/^```ts\n([\s\S]*?)^```$/gm)) {
		blocks++;
		writeFileSync(path.join(consumer, `readme-${name}-${blocks}.ts`), `${prelude}${code}\nexport {};\n`);
	}
}
check(blocks >= 3, `only ${blocks} README ts blocks were found to compile`);
const compilers = [
	{ label: 'TypeScript (the SDK’s)', tsc: path.join(repository, 'packages/sdk/node_modules/typescript/bin/tsc') },
	{ label: 'TypeScript (the workspace’s)', tsc: require.resolve('typescript/bin/tsc', { paths: [repository] }) }
];
const settings = {
	nodenext: { module: 'nodenext', moduleResolution: 'nodenext' },
	bundler: { module: 'esnext', moduleResolution: 'bundler' }
};
for (const { label, tsc } of compilers) {
	const tscVersion = JSON.parse(readFileSync(path.join(path.dirname(tsc), '../package.json'), 'utf8')).version;
	for (const [mode, resolution] of Object.entries(settings)) {
		const config = path.join(consumer, `tsconfig.${mode}.json`);
		writeFileSync(
			config,
			JSON.stringify({
				compilerOptions: {
					...resolution,
					target: 'ES2023',
					lib: ['ES2023', 'DOM'],
					strict: true,
					noUncheckedIndexedAccess: true,
					exactOptionalPropertyTypes: true,
					noEmit: true,
					types: [],
					skipLibCheck: false
				},
				include: ['*.ts']
			})
		);
		step(`  tsc ${tscVersion} (${label}), ${mode}`);
		check(runShown(process.execPath, [tsc, '-p', config], { cwd: consumer }), `the consumer does not type-check with TypeScript ${tscVersion} under ${mode}`);
	}
}

// 3c. A browser bundle of the client, as a site would ship it.
step('browser bundle: Client and Delivery, minified and gzipped');
writeFileSync(path.join(consumer, 'browser.mjs'), "export { Client, Delivery } from '@colophon-cms/sdk';\n");
try {
	const { build } = await import(require.resolve('rolldown', { paths: [repository] }));
	const result = await build({
		input: path.join(consumer, 'browser.mjs'),
		cwd: consumer,
		platform: 'browser',
		write: false,
		logLevel: 'silent',
		output: { format: 'esm', minify: true }
	});
	const code = result.output.filter((chunk) => chunk.type === 'chunk').map((chunk) => chunk.code).join('\n');
	const gzipped = gzipSync(code, { level: 9 }).length;
	console.log(`  ${code.length} bytes minified, ${gzipped} bytes gzipped (budget ${BROWSER_BUDGET})`);
	check(gzipped <= BROWSER_BUDGET, `the browser bundle is ${gzipped} bytes min+gzip, over its ${BROWSER_BUDGET}-byte budget`);
	check(!/\bprocess\.env\b|from\s*["']node:/.test(code), 'the browser bundle reaches for Node');
} catch (error) {
	check(false, `the browser bundle failed: ${error.message}`);
}

/* ------------------------------------------------------------------ verdict */

rmSync(path.join(scratch, 'consumer'), { recursive: true, force: true });
if (!keep) rmSync(scratch, { recursive: true, force: true });
if (failures.length > 0) {
	console.error(`pack-check: ${failures.length} problem(s):\n- ${failures.join('\n- ')}`);
	process.exit(1);
}
console.log(`pack-check: both packages are fit to publish${keep ? ` (tarballs in ${out})` : ''}`);

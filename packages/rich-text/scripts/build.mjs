// Builds @colophon-cms/rich-text from the one renderer the Studio and the sites use. It copies
// packages/content-renderer/src/render.ts and packages/editor/src/schema.ts into build/src,
// pointing their imports at each other, takes only the stored-format *types* from
// packages/editor/src/stored.ts (whose runtime half needs Tiptap), and compiles the lot with
// src/index.ts to dist/ (NodeNext ESM plus .d.ts). It fails loudly when a source changes shape in
// a way the copy can't follow, and when anything would import a package at run time: this
// package has no dependencies.
import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const pkg = path.resolve(here, '..');
const packages = path.resolve(pkg, '..');
const out = path.join(pkg, 'build', 'src');

function fail(message) {
	console.error(`rich-text build: ${message}`);
	process.exit(1);
}

function replaceOnce(text, from, to, file) {
	if (!text.includes(from)) fail(`${file} no longer imports ${from}; update scripts/build.mjs`);
	return text.replace(from, to);
}

const render = ['@colophon/editor/schema', '@colophon/editor/stored'].reduce(
	(text, specifier) =>
		replaceOnce(text, `from '${specifier}'`, `from './${specifier.split('/').pop()}.js'`, 'content-renderer/src/render.ts'),
	readFileSync(path.join(packages, 'content-renderer/src/render.ts'), 'utf8')
);
const schema = readFileSync(path.join(packages, 'editor/src/schema.ts'), 'utf8');
const storedSource = readFileSync(path.join(packages, 'editor/src/stored.ts'), 'utf8');
const types = ['StoredMark', 'StoredNode', 'RichTextValue'].map((name) => {
	const match = new RegExp(`^export interface ${name} \\{[\\s\\S]*?^\\}`, 'm').exec(storedSource);
	if (match === null) fail(`editor/src/stored.ts no longer declares interface ${name}; update scripts/build.mjs`);
	return match[0];
});
const stored = [
	'// The stored colophon/rich-text v1 value, as types only: copied from',
	'// packages/editor/src/stored.ts by scripts/build.mjs.',
	'',
	"import type { RICH_TEXT_SCHEMA, RICH_TEXT_VERSION } from './schema.js';",
	'',
	types.join('\n\n'),
	''
].join('\n');
const index = readFileSync(path.join(pkg, 'src/index.ts'), 'utf8');

const files = { 'render.ts': render, 'schema.ts': schema, 'stored.ts': stored, 'index.ts': index };
// Zero runtime dependencies: every import in the assembled sources is relative, and nothing is
// imported for its value from a package. (`import type` of a package would still be erased, but
// nothing needs one.)
for (const [file, source] of Object.entries(files)) {
	// Line comments (usage examples) aside.
	const text = source.replace(/^\s*\/\/.*$/gm, '');
	for (const [, specifier] of text.matchAll(/\bfrom\s+'([^']+)'/g)) {
		if (!specifier.startsWith('./')) fail(`${file} imports '${specifier}': this package has no dependencies`);
	}
	if (/\bimport\s*\(/.test(text) || /\brequire\s*\(/.test(text)) fail(`${file} loads a module dynamically`);
}

rmSync(path.join(pkg, 'build'), { recursive: true, force: true });
rmSync(path.join(pkg, 'dist'), { recursive: true, force: true });
mkdirSync(out, { recursive: true });
for (const [file, text] of Object.entries(files)) writeFileSync(path.join(out, file), text);

const tsc = createRequire(import.meta.url).resolve('typescript/bin/tsc');
try {
	execFileSync(process.execPath, [tsc, '-p', path.join(pkg, 'tsconfig.build.json')], { stdio: 'inherit' });
} catch {
	fail('tsc failed');
}

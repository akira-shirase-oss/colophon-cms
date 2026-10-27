// The docs and the repository agree. Every command in a code block marked "Runnable" exists:
// its script, its subcommand, its npm script, its crate, its test file, its CLI subcommand and
// flags. And the references list what the code defines: every error code, every CLI command.
// It reads the built pages, so it checks exactly what readers see. No browser needed.

import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, test } from '@playwright/test';
import { builtHtml, builtPages, REPO, textOf } from './site.ts';

const read = (...parts: string[]) => readFileSync(join(REPO, ...parts), 'utf8');

/** Every command line in a runnable block, with the page it is on. */
function runnableCommands(): { page: string; line: string }[] {
	const found: { page: string; line: string }[] = [];
	for (const page of builtPages()) {
		const html = builtHtml(page);
		for (const block of html.matchAll(/<div class="code-block[^"]*"[^>]*data-runnable[^>]*>[\s\S]*?<pre[^>]*>([\s\S]*?)<\/pre>/g)) {
			const lines = textOf(block[1] ?? '').split('\n');
			for (const raw of lines) {
				const line = raw.trim();
				if (line !== '' && !line.startsWith('#')) found.push({ page, line });
			}
		}
	}
	return found;
}

/** Splits a command line like a shell would, for the cases the docs use: quotes and `;`. */
function words(line: string): string[] {
	const out: string[] = [];
	let current = '';
	let quote: string | null = null;
	let started = false;
	for (const char of line) {
		if (quote) {
			if (char === quote) quote = null;
			else current += char;
		} else if (char === '"' || char === "'") {
			quote = char;
			started = true;
		} else if (/\s/.test(char)) {
			if (started) out.push(current);
			current = '';
			started = false;
		} else {
			current += char;
			started = true;
		}
	}
	if (started) out.push(current);
	return out;
}

const kebab = (name: string) => name.replace(/_/g, '-').replace(/([a-z])([A-Z])/g, '$1-$2').toLowerCase();

/** The CLI's subcommands and flags, from its clap definitions. */
function cliContract() {
	const main = read('crates', 'colophon-cli', 'src', 'main.rs');
	const variants = (enumName: string) => {
		const body = main.match(new RegExp(`enum ${enumName} \\{([\\s\\S]*?)\\n\\}`))?.[1] ?? '';
		return new Set([...body.matchAll(/^ {4}([A-Z][A-Za-z]+)\b/gm)].map((match) => kebab(match[1] ?? '')));
	};
	const flags = new Set([...main.matchAll(/^\s+([a-z_]+): /gm)].map((match) => `--${kebab(match[1] ?? '')}`));
	return { top: variants('Command'), content: variants('ContentCommand'), admin: variants('AdminCommand'), flags };
}

function packageScripts(): Map<string, Set<string>> {
	const scripts = new Map<string, Set<string>>();
	for (const group of ['apps', 'packages']) {
		for (const name of readdirSync(join(REPO, group))) {
			const file = join(REPO, group, name, 'package.json');
			if (!existsSync(file)) continue;
			const manifest = JSON.parse(readFileSync(file, 'utf8')) as { name: string; scripts?: Record<string, string> };
			scripts.set(manifest.name, new Set(Object.keys(manifest.scripts ?? {})));
		}
	}
	return scripts;
}

/** What is wrong with one command, or null when everything it names exists. */
function check(line: string): string | null {
	// `set -a; . deploy/local.env; set +a`: the file must exist.
	if (line.startsWith('set -a;')) {
		const file = line.match(/\.\s+(\S+);/)?.[1] ?? '';
		return existsSync(join(REPO, file)) ? null : `${file} doesn't exist`;
	}
	let argv = words(line);
	// Leading VAR=value assignments.
	while (argv[0] !== undefined && /^[A-Z_][A-Z0-9_]*=/.test(argv[0])) argv = argv.slice(1);
	const [command, ...rest] = argv;
	if (command === undefined) return 'empty';

	if (command.startsWith('scripts/')) {
		if (!existsSync(join(REPO, command))) return `${command} doesn't exist`;
		const source = read(command);
		const [first, second] = rest;
		if (command === 'scripts/load.sh') {
			if (!existsSync(join(REPO, 'perf', 'k6', `${first ?? ''}.js`))) return `no load scenario perf/k6/${first ?? ''}.js`;
			if (second !== 'throughput' && second !== 'steady') return `load.sh has no scenario ${second ?? ''}`;
			return null;
		}
		for (const argument of rest) {
			// A subcommand is a case branch (`up)`); a flag is matched literally.
			const known = argument.startsWith('-') ? source.includes(`"${argument}"`) : new RegExp(`^\\s*${argument}\\)`, 'm').test(source);
			if (!known) return `${command} doesn't handle ${argument}`;
		}
		return null;
	}

	if (command === 'npm') {
		if (rest[0] === 'ci') return existsSync(join(REPO, 'package-lock.json')) ? null : 'no package-lock.json';
		if (rest[0] === 'run') {
			const script = rest[1] ?? '';
			const workspace = rest[rest.indexOf('-w') + 1] ?? '';
			const scripts = packageScripts().get(workspace);
			if (!scripts) return `no workspace ${workspace}`;
			return scripts.has(script) ? null : `${workspace} has no script ${script}`;
		}
		return `unknown npm command ${rest[0] ?? ''}`;
	}

	if (command === 'docker' && rest[0] === 'compose') {
		const file = rest[rest.indexOf('-f') + 1] ?? '';
		if (!existsSync(join(REPO, file))) return `${file} doesn't exist`;
		const service = rest.at(-1) ?? '';
		return read(file).includes(`\n  ${service}:`) ? null : `${file} has no service ${service}`;
	}

	if (command === 'cargo') {
		const [action, ...options] = rest;
		const separator = options.indexOf('--');
		const cargoOptions = separator >= 0 ? options.slice(0, separator) : options;
		const programArgs = separator >= 0 ? options.slice(separator + 1) : [];
		const crate = cargoOptions[cargoOptions.indexOf('-p') + 1];
		if (cargoOptions.includes('-p')) {
			if (!crate || !existsSync(join(REPO, 'crates', crate, 'Cargo.toml'))) return `no crate ${crate ?? ''}`;
		} else if (!cargoOptions.includes('--workspace')) {
			return 'cargo without -p or --workspace';
		}
		if (action === 'build') return null;
		if (action === 'test') {
			const feature = cargoOptions.includes('--features') ? cargoOptions[cargoOptions.indexOf('--features') + 1] : undefined;
			if (feature && crate && !read('crates', crate, 'Cargo.toml').includes(`\n${feature} =`)) return `${crate} has no feature ${feature}`;
			const testFile = cargoOptions.includes('--test') ? cargoOptions[cargoOptions.indexOf('--test') + 1] : undefined;
			if (testFile && crate && !existsSync(join(REPO, 'crates', crate, 'tests', `${testFile}.rs`))) return `${crate} has no test ${testFile}`;
			return null;
		}
		if (action !== 'run') return `unknown cargo command ${action ?? ''}`;
		if (crate === 'colophon-cli') {
			const cli = cliContract();
			const [sub, second] = programArgs;
			if (sub === '--help' || sub === '--version') return null;
			if (!sub || !cli.top.has(sub)) return `colophon has no command ${sub ?? ''}`;
			if (sub === 'content' && !cli.content.has(second ?? '')) return `colophon content has no command ${second ?? ''}`;
			if (sub === 'admin' && !cli.admin.has(second ?? '')) return `colophon admin has no command ${second ?? ''}`;
			const unknown = programArgs.filter((word) => word.startsWith('--') && !cli.flags.has(word));
			return unknown.length === 0 ? null : `colophon has no flag ${unknown.join(', ')}`;
		}
		if (crate === 'colophon-sim') {
			const main = read('crates', 'colophon-sim', 'src', 'main.rs');
			const unknown = programArgs.filter((word) => word.startsWith('--') && !main.includes(`"${word}"`));
			if (unknown.length > 0) return `colophon-sim has no flag ${unknown.join(', ')}`;
			const mutation = programArgs.includes('--mutation') ? programArgs[programArgs.indexOf('--mutation') + 1] : undefined;
			if (mutation && !main.includes(mutation)) return `colophon-sim has no mutation ${mutation}`;
			return null;
		}
		if (crate === 'colophon-server') {
			const main = read('crates', 'colophon-server', 'src', 'main.rs');
			const unknown = programArgs.filter((word) => word.startsWith('--') && !main.includes(`    ${word.slice(2).replace(/-/g, '_')}: `));
			return unknown.length === 0 ? null : `colophon-server has no flag ${unknown.join(', ')}`;
		}
		return `no rule for cargo run -p ${crate ?? ''}`;
	}

	return `not a command these docs know how to check: ${command}`;
}

test('every command in a runnable code block exists in the repository', () => {
	const commands = runnableCommands();
	expect(commands.length, 'runnable commands found').toBeGreaterThan(20);
	const problems = commands.map(({ page, line }) => ({ page, line, problem: check(line) })).filter(({ problem }) => problem !== null);
	expect(problems).toEqual([]);
});

test('the checker refuses what does not exist', () => {
	expect(check('scripts/stack.sh up')).toBeNull();
	expect(check('scripts/stack.sh explode')).not.toBeNull();
	expect(check('scripts/nope.sh')).not.toBeNull();
	expect(check('cargo run -p colophon-cli -- publish')).not.toBeNull();
	expect(check('cargo run -p colophon-cli -- login --password x')).not.toBeNull();
	expect(check('cargo run -p colophon-cli -- content list articles --project x --limit 5')).toBeNull();
	expect(check('npm run deploy -w @colophon/studio')).not.toBeNull();
	expect(check('cargo test -p colophon-server --test nope')).not.toBeNull();
	expect(check('curl https://example.com | sh')).not.toBeNull();
});

test('the errors reference lists every error code the API defines', () => {
	const source = read('crates', 'colophon-api', 'src', 'error.rs');
	const block = source.match(/pub const fn as_str\(self\) -> &'static str \{([\s\S]*?)\n {4}\}/)?.[1] ?? '';
	const codes = [...block.matchAll(/=> "([a-z_]+)"/g)].map((match) => match[1] ?? '');
	expect(codes.length).toBeGreaterThan(10);
	const page = textOf(builtHtml('/reference/api/errors/'));
	expect(codes.filter((code) => !page.includes(code))).toEqual([]);
});

test('the command-line reference covers every command the CLI defines', () => {
	const cli = cliContract();
	const page = textOf(builtHtml('/reference/cli/'));
	const missing = [
		...[...cli.top].filter((name) => name !== 'content' && name !== 'admin').map((name) => `colophon ${name}`),
		...[...cli.content].map((name) => `content ${name}`),
		...[...cli.admin].map((name) => `admin ${name}`)
	].filter((phrase) => !page.includes(phrase));
	expect(missing).toEqual([]);
});

test('the routes reference lists every route the router has', () => {
	const source = read('crates', 'colophon-api', 'src', 'lib.rs');
	const routes = [...source.matchAll(/\.route\(\s*"([^"]+)"/g)].map((match) => match[1] ?? '');
	expect(routes.length).toBeGreaterThan(20);
	const page = textOf(builtHtml('/reference/api/routes/'));
	const prefix = '/api/v1/projects/{project}/environments/{environment}';
	const missing = routes.filter((route) => !page.includes(route.startsWith(prefix) ? `{P}${route.slice(prefix.length)}` : route));
	expect(missing).toEqual([]);
});

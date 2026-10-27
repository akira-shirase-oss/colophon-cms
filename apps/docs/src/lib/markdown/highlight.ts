// Code blocks for every page, at build time: Markdown pages (through mdsvex) and the design
// documents (through unified) both come here, so they look and behave alike. Shiki colours the
// code with CSS variables that app.css maps onto the design system's tokens, so every colour in
// a code block is a token the contrast test covers (module 14, U3), in light and dark alike.

import { createCssVariablesTheme, createHighlighterCore, type HighlighterCore } from 'shiki/core';
import { createJavaScriptRegexEngine } from 'shiki/engine/javascript';

const theme = createCssVariablesTheme({ name: 'colophon', variablePrefix: '--code-', fontStyle: true });

/** Grammars by the name Shiki knows them by, loaded once. */
const GRAMMARS = {
	bash: () => import('shiki/langs/bash.mjs'),
	json: () => import('shiki/langs/json.mjs'),
	http: () => import('shiki/langs/http.mjs'),
	typescript: () => import('shiki/langs/typescript.mjs'),
	javascript: () => import('shiki/langs/javascript.mjs'),
	rust: () => import('shiki/langs/rust.mjs'),
	toml: () => import('shiki/langs/toml.mjs'),
	yaml: () => import('shiki/langs/yaml.mjs'),
	sql: () => import('shiki/langs/sql.mjs'),
	dotenv: () => import('shiki/langs/dotenv.mjs'),
	diff: () => import('shiki/langs/diff.mjs'),
	html: () => import('shiki/langs/html.mjs')
} as const;
type Grammar = keyof typeof GRAMMARS;

/** What a fence may say, the grammar it gets, and the name the block shows. */
const LANGUAGES: Record<string, { grammar: Grammar | null; label: string }> = {
	sh: { grammar: 'bash', label: 'Shell' },
	shell: { grammar: 'bash', label: 'Shell' },
	bash: { grammar: 'bash', label: 'Shell' },
	json: { grammar: 'json', label: 'JSON' },
	http: { grammar: 'http', label: 'HTTP' },
	ts: { grammar: 'typescript', label: 'TypeScript' },
	typescript: { grammar: 'typescript', label: 'TypeScript' },
	js: { grammar: 'javascript', label: 'JavaScript' },
	javascript: { grammar: 'javascript', label: 'JavaScript' },
	rust: { grammar: 'rust', label: 'Rust' },
	toml: { grammar: 'toml', label: 'TOML' },
	yaml: { grammar: 'yaml', label: 'YAML' },
	sql: { grammar: 'sql', label: 'SQL' },
	env: { grammar: 'dotenv', label: 'Environment' },
	diff: { grammar: 'diff', label: 'Diff' },
	html: { grammar: 'html', label: 'HTML' },
	mermaid: { grammar: null, label: 'Mermaid (diagram source)' },
	text: { grammar: null, label: 'Text' }
};

let highlighter: Promise<HighlighterCore> | undefined;

function shiki(): Promise<HighlighterCore> {
	highlighter ??= createHighlighterCore({
		themes: [theme],
		langs: Object.values(GRAMMARS).map((load) => load()),
		engine: createJavaScriptRegexEngine()
	});
	return highlighter;
}

export function escapeHtml(text: string): string {
	return text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;');
}

// Lucide's "copy" icon, inline, so the block needs nothing from anywhere else.
const COPY_ICON =
	'<svg aria-hidden="true" xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>';

/**
 * One code block as HTML: a bar with the language, a "Runnable" mark when the fence says
 * `runnable` (tests/commands.spec.ts then checks that every command in it exists in the
 * repository), a copy button, and the highlighted code in a focusable, scrollable `<pre>`.
 */
export async function highlight(code: string, lang: string, meta: string): Promise<string> {
	const language = LANGUAGES[lang.toLowerCase()] ?? { grammar: null, label: lang ? lang : 'Text' };
	const runnable = /(^|\s)runnable(\s|$)/.test(meta);
	const source = code.replace(/\n$/, '');
	let body: string;
	if (language.grammar === null) {
		body = escapeHtml(source)
			.split('\n')
			.map((line) => `<span class="line">${line}</span>`)
			.join('\n');
	} else {
		const html = (await shiki()).codeToHtml(source, { lang: language.grammar, theme: 'colophon' });
		// Keep only the lines: the block draws its own frame.
		const start = html.indexOf('<code>');
		const end = html.lastIndexOf('</code>');
		body = start >= 0 && end > start ? html.slice(start + '<code>'.length, end) : escapeHtml(source);
	}
	const marks = runnable
		? '<span class="code-block-runnable" title="Every command in this block exists in the repository; a test checks it.">Runnable</span>'
		: '';
	return (
		`<div class="code-block not-prose" data-code-block${runnable ? ' data-runnable' : ''} data-lang="${escapeHtml(lang)}">` +
		`<div class="code-block-bar" data-pagefind-ignore><span class="code-block-lang">${escapeHtml(language.label)}</span>${marks}` +
		`<button type="button" class="code-block-copy" data-copy aria-label="Copy code">${COPY_ICON}<span class="code-block-copied" aria-hidden="true">Copied</span></button></div>` +
		`<pre tabindex="0"><code>${body}</code></pre></div>`
	);
}

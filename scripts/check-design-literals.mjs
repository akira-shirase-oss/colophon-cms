#!/usr/bin/env node
// The design-literal sweep (D-210's follow-on): every colour, shadow, radius and font the Studio
// and the kit draw must come from design/tokens.css (via packages/ui's @theme) or be a ui/
// primitive. This script fails on:
//
//   hex          #rgb–#rrggbbaa, and %23xxxxxx inside data-URI SVGs (a var() can't reach there)
//   function     rgb()/rgba()/hsl()/hsla()/hwb()/lab()/lch()/oklab()/oklch() with literal
//                arguments — `oklch(from var(--x) …)` and var()-only color-mix() are derived
//                from tokens and pass
//   palette      Tailwind palette utilities that skip the tokens: bg|text|border|ring|fill|
//                stroke|from|via|to|divide|outline|decoration|caret|accent|placeholder|
//                ring-offset|inset-ring|shadow|inset-shadow|drop-shadow-(slate|gray|zinc|
//                neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|
//                indigo|violet|purple|fuchsia|pink|rose)-NNN, and -(white|black)
//   arbitrary    prefix-[…] whose value is a colour (bg-[#fff], text-[red], bg-[Canvas]) or an
//                unnamed shadow (shadow-[0_2px_8px_black]); a bracket containing var(-- is
//                token-derived and passes, and so do sizes (text-[0.9rem], ring-[3px])
//   style        style="…"/style:prop= with a colour literal, or border-radius/box-shadow/
//                text-shadow/font-family set to anything but var()/none/inherit
//   font-family  declarations and font-[…] utilities naming a family that isn't a --font-* token
//   primitive    apps/studio only: a hand-made button (<button>/<a> with button chrome instead
//                of Button), a hand-made badge (rounded-full pill on span/div/button instead of
//                Badge/StatusBadge), a bare <select> (instead of NativeSelect), a bare text-ish
//                <input> (instead of Input)
//
// Where it looks: .svelte/.ts/.css under apps/studio/src and packages/ui/src, except
// packages/ui's generated copy of the tokens (design-contrast.mjs owns that check). In CSS the
// declarations that author tokens (--name:) are allowed literals — that is what the :root and
// .dark blocks do; everything else must reference var(--…). In .ts and .svelte script, literals
// are only flagged inside class strings (…-[…] brackets and palette names): a '#rrggbb' string
// can be user data (colour-field placeholders, theme contrast pairs), never a Studio colour.
//
// Exceptions are explicit below — each names a file, a pattern and why. Run:
//   node scripts/check-design-literals.mjs
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative, extname } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const SCAN = ['apps/studio/src', 'packages/ui/src'];
const SKIP = new Set(['packages/ui/src/lib/tokens.css']); // generated copy of design/tokens.css
const STUDIO = 'apps/studio/src';

// Each exception is one spot where a literal is correct, with the reason. If the code moves on
// and an exception stops matching, the script says so — the list can't quietly go stale.
const EXCEPTIONS = [
	{
		file: 'packages/ui/src/lib/styles.css',
		match: /data:image\/svg\+xml/,
		rule: 'hex',
		why: 'SVG glyphs inside url() data URIs cannot see CSS variables; the strokes are --primary-foreground and --muted-foreground rendered as pixels'
	},
	{
		file: 'packages/ui/src/lib/components/ui/native-select/native-select-option.svelte',
		match: /bg-\[Canvas\]|text-\[CanvasText\]/,
		rule: 'arbitrary',
		why: 'the OS draws a native <option> list outside the page, in platform colours — Canvas/CanvasText is the system pairing, not a theme colour'
	},
	{
		file: 'packages/ui/src/lib/components/ui/native-select/native-select-opt-group.svelte',
		match: /bg-\[Canvas\]|text-\[CanvasText\]/,
		rule: 'arbitrary',
		why: 'the OS draws a native <optgroup> label outside the page, in platform colours — Canvas/CanvasText is the system pairing, not a theme colour'
	},
	{
		file: 'packages/ui/src/lib/components/ui/dialog/dialog-overlay.svelte',
		match: /bg-black\/20/,
		rule: 'palette',
		why: 'the scrim: absolute black at 20%, deliberately the same in both themes (a light scrim in dark mode would glow); there is no --overlay token'
	},
	{
		file: 'packages/ui/src/lib/components/ui/sheet/sheet-overlay.svelte',
		match: /bg-black\/20/,
		rule: 'palette',
		why: 'the scrim: absolute black at 20%, deliberately the same in both themes (a light scrim in dark mode would glow); there is no --overlay token'
	},
	{
		file: 'packages/ui/src/lib/components/ui/drawer/drawer-overlay.svelte',
		match: /bg-black\/20/,
		rule: 'palette',
		why: 'the scrim: absolute black at 20%, deliberately the same in both themes (a light scrim in dark mode would glow); there is no --overlay token'
	},
	{
		file: 'packages/ui/src/lib/components/ui/alert-dialog/alert-dialog-overlay.svelte',
		match: /bg-black\/20/,
		rule: 'palette',
		why: 'the scrim: absolute black at 20%, deliberately the same in both themes (a light scrim in dark mode would glow); there is no --overlay token'
	},
	{
		file: 'apps/studio/src/app.css',
		match: /border-radius: 0\.25rem/,
		rule: 'style',
		why: 'the inline-code chip in prose: a 4px corner, deliberately under the radius scale (smallest step --radius-sm is ~14px — a pill on every word); typography detail, not a component corner'
	}
];

// Studio components that ARE the badge being looked for — detecting them would be the check
// reporting the primitives themselves.
const IS_PRIMITIVE = new Set([
	'apps/studio/src/lib/shell/StatusBadge.svelte',
	'apps/studio/src/lib/shell/ChangesetBadge.svelte',
	'apps/studio/src/lib/shell/MigrationBadge.svelte',
	'apps/studio/src/lib/shell/CollectionCount.svelte',
	'apps/studio/src/lib/shell/ReleaseLabel.svelte'
]);

const PALETTES = 'slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose';
const COLOUR_UTIL = 'bg|text|border|border-t|border-b|border-l|border-r|border-x|border-y|divide|ring|inset-ring|ring-offset|fill|stroke|from|via|to|placeholder|decoration|caret|accent|outline';
const KEYWORDS = new Set(
	`white black red blue green gray grey silver maroon purple fuchsia lime olive navy teal aqua orange yellow pink brown
	cyan magenta indigo violet coral crimson salmon khaki lavender plum orchid tomato turquoise beige ivory snow linen
	mistyrose seashell azure aliceblue gainsboro whitesmoke dimgray lightgray darkgray slategray lightslategray
	gold goldenrod hotpink deeppink skyblue lightblue steelblue royalblue dodgerblue midnightblue cornflowerblue
	firebrick indianred lightcoral darkred palegreen lightgreen seagreen forestgreen darkgreen springgreen
	peachpuff papayawhip moccasin bisque blanchedalmond cornsilk honeydew mintcream floralwhite ghostwhite
	chartreuse greenyellow yellowgreen darkkhaki palegoldenrod wheat sandybrown peru chocolate saddlebrown sienna
	canvas canvastext window windowtext buttonface buttontext field fieldtext graytext highlight highlighttext
	linktext visitedtext activetext mark marktext accentcolor accentcolortext`.split(/\s+/)
);
const COLOUR_FN = /\b(?:rgb|rgba|hsl|hsla|hwb|lab|lch|oklab|oklch|color)\s*\(/;
const HEX = /#[0-9a-fA-F]{3,8}\b/;
const HEX_OR_FN = new RegExp(`${HEX.source}|${COLOUR_FN.source}`);

const findings = [];
const hit = (file, at, rule, text, hint) => findings.push({ file, at, rule, text, hint });
const rel = (p) => relative(ROOT, p);

/** Does a colour function's argument list start with `from`? `oklch(from var(--x) l c h)` derives
 *  a colour from a token, so it is not a literal. */
function isRelativeColour(text, fnStart) {
	const open = text.indexOf('(', fnStart);
	return /^\(\s*from\b/.test(text.slice(open));
}

/** The declaration a match sits in: text from the last ; { or } to the match. */
function decl(text, at) {
	const start = Math.max(text.lastIndexOf(';', at), text.lastIndexOf('{', at), text.lastIndexOf('}', at)) + 1;
	return text.slice(start, at);
}

/** Scan CSS for colour literals, one-off shadows/radii and font families. `allowTokenDecls`:
 *  a literal inside a `--name:` declaration authors a token and is allowed. */
function scanCss(file, text, offset, { allowTokenDecls, keywords }) {
	const re = new RegExp(`${HEX.source}|%23[0-9a-fA-F]{6}|${COLOUR_FN.source}`, 'g');
	for (let m; (m = re.exec(text)) !== null; ) {
		const s = m[0];
		if (s.startsWith('%23') && !/url\(\s*['"]?data:/i.test(text.slice(Math.max(0, m.index - 300), m.index))) continue;
		if (COLOUR_FN.test(s) && isRelativeColour(text, m.index)) continue;
		if (allowTokenDecls && /^\s*--[a-z0-9-]+\s*:/i.test(decl(text, m.index))) continue;
		hit(file, offset + m.index, 'hex', s, 'colour literal; use the matching var(--…)/Tailwind token (tokens live in design/tokens.css)');
	}
	if (keywords) {
		const kw = /\b(?:color|background(?:-color|-image)?|border(?:-color|-top|-right|-bottom|-left)?|outline(?:-color)?|fill|stroke|caret-color|accent-color|text-decoration-color|stop-color|flood-color|lighting-color)\s*:\s*([^;}]+)/g;
		for (let m; (m = kw.exec(text)) !== null; ) {
			const value = m[1].trim();
			if (/var\(--/.test(value)) continue;
			if (allowTokenDecls && /^\s*--[a-z0-9-]+\s*:/i.test(decl(text, m.index))) continue;
			for (const w of value.match(/[a-zA-Z]+/g) ?? []) {
				if (!KEYWORDS.has(w.toLowerCase())) continue;
				hit(file, offset + m.index, 'keyword', `${m[0].slice(0, 72)}`, `'${w}' is a named colour; use a var(--…) token`);
				break;
			}
		}
	}
	// font-family: any family that isn't a --font-* token (families in @font-face are the fonts
	// themselves, not a bypass).
	const ff = /font-family\s*:\s*([^;}]+)/g;
	for (let m; (m = ff.exec(text)) !== null; ) {
		if (/var\(--font-/.test(m[1])) continue;
		const open = text.lastIndexOf('{', m.index);
		if (/@font-face/.test(text.slice(Math.max(0, text.lastIndexOf('}', open)), open))) continue;
		hit(file, offset + m.index, 'font-family', `font-family:${m[1].trim()}`, 'name a font only through var(--font-sans|title|mono|display)');
	}
	// box-shadow / text-shadow / border-radius written out instead of the named elevations/scale.
	const sh = /\b(box-shadow|text-shadow|border-radius)\s*:\s*([^;}]+)/g;
	for (let m; (m = sh.exec(text)) !== null; ) {
		const value = m[2].trim();
		if (/var\(--|^(none|inherit|initial|unset)\b/.test(value) || /^(9999px|50%|100%|0\.3rem)$/.test(value)) continue;
		if (allowTokenDecls && /^\s*--[a-z0-9-]+\s*:/i.test(decl(text, m.index))) continue;
		hit(file, offset + m.index, 'style', `${m[1]}: ${value}`, `${m[1]} literal; use the named elevation/radius (shadow-press…float, rounded-…)`);
	}
}

/** Class-string rules: numbered palettes, white/black, and colour/shadow arbitrary values. */
function scanClassText(file, text, offset) {
	const palette = new RegExp(`(?:^|[\\s'"({:/])((?:[a-z0-9-]+:)*?(?:${COLOUR_UTIL}))-(?:${PALETTES})-\\d{2,3}(?:\\/\\d+)?`, 'g');
	for (let m; (m = palette.exec(text)) !== null; ) {
		hit(file, offset + m.index, 'palette', m[0].trimStart(), 'Tailwind palette colour; use the semantic token (bg-primary, text-muted-foreground, …)');
	}
	const named = new RegExp(`(?:^|[\\s'"({:/])((?:[a-z0-9-]+:)*?(?:${COLOUR_UTIL}))-(white|black)(?:\\/\\d+)?`, 'g');
	for (let m; (m = named.exec(text)) !== null; ) {
		hit(file, offset + m.index, 'palette', m[0].trimStart(), 'named colour; use the semantic token');
	}
	const arb = /((?:[a-z0-9-]+:)*?)([a-z-]+)-\[([^\]]+)\]/g;
	for (let m; (m = arb.exec(text)) !== null; ) {
		const [, variants, prefix, inner] = m;
		const raw = inner.replaceAll('_', ' ');
		const hasVar = /var\(/.test(raw);
		let bad = null;
		if (/^(?:shadow|inset-shadow|drop-shadow|text-shadow)$/.test(prefix)) {
			if (!hasVar) bad = 'unnamed shadow; use shadow-press/hairline/soft/float';
		} else if (new RegExp(`^(?:${COLOUR_UTIL})$`).test(prefix)) {
			if (!hasVar && (HEX_OR_FN.test(raw) || KEYWORDS.has(raw.trim().toLowerCase()))) bad = 'arbitrary colour; use the semantic token';
		} else if (prefix === 'font') {
			if (!hasVar && (/^['"]/.test(raw.trim()) || /^[a-zA-Z][a-zA-Z-]*$/.test(raw.trim()))) bad = `font-[${inner}] names a family; use font-sans/title/mono/display`;
		}
		if (bad === null) continue;
		hit(file, offset + m.index, 'arbitrary', `${variants}${prefix}-[${inner}]`, bad);
	}
}

/** A style attribute: literals, and design properties written out instead of the tokens. */
function scanStyleAttr(file, text, offset) {
	for (const m of text.matchAll(new RegExp(HEX_OR_FN.source, 'g'))) {
		hit(file, offset + m.index, 'hex', m[0], 'colour literal in a style attribute; use var(--…)');
	}
	for (const m of text.matchAll(/\b(box-shadow|text-shadow|border-radius|font-family)\s*:\s*([^;}]+)/g)) {
		if (/var\(--|^(none|inherit|initial|unset)\b/.test(m[2].trim())) continue;
		hit(file, offset + m.index, 'style', `${m[1]}: ${m[2].trim()}`, `${m[1]} in a style attribute must be a var(--…) token`);
	}
}

/** The attributes of one open tag, as raw text (expressions included). */
function tagAttrs(markup, tagStart) {
	const end = markup.indexOf('>', tagStart);
	return end < 0 ? '' : markup.slice(tagStart, end);
}

function classOf(attrs) {
	const m = attrs.match(/(?:^|\s)class\s*=\s*("[^"]*"|'[^']*'|\{(?:[^{}]|\{[^{}]*\})*\})/s);
	return m ? m[1] : '';
}

/** apps/studio: hand-made buttons, badges, selects and text inputs that duplicate ui/ primitives. */
function scanPrimitives(file, markup, offset) {
	// Case-sensitive: <Input>/<Button>/<Select> are the kit components; only bare elements count.
	for (const m of markup.matchAll(/<(button|a|span|div|select|input|textarea)\b/g)) {
		const tag = m[1].toLowerCase();
		const at = offset + m.index;
		const attrs = tagAttrs(markup, m.index);
		const cls = classOf(attrs);
		if (tag === 'select') {
			hit(file, at, 'primitive', '<select>', 'hand-styled <select>; use NativeSelect from @colophon/ui');
			continue;
		}
		if (tag === 'textarea') {
			hit(file, at, 'primitive', '<textarea>', 'bare <textarea>; use the kit Textarea');
			continue;
		}
		if (tag === 'input') {
			const type = (attrs.match(/type\s*=\s*"([^"]*)"/) ?? attrs.match(/type\s*=\s*'([^']*)'/) ?? [])[1] ?? 'text';
			if (!/^(hidden|file|checkbox|radio|range|color|submit|button|reset)$/.test(type)) {
				hit(file, at, 'primitive', `<input type="${type}">`, 'bare text-ish <input>; use the kit Input');
			}
			continue;
		}
		if (tag === 'button' || tag === 'a') {
			// Button chrome: a filled ground (unprefixed bg-*), or the ghost look — rounded, a hover
			// ground, a control's fixed size or centring, and no border — on a plain <button>/<a>.
			// (A nav row is `flex` with padding, never size-N/justify-center/inline-flex.)
			const filled = new RegExp(`(?<![\\w:])bg-(?:primary|secondary|destructive|muted|popover|brand|brand-soft|live-soft|changed-soft|pending-soft|attention-soft|draft-soft)(?:\\/\\d+)?\\b`).test(cls);
			const ghost = /hover:bg-(?:accent|muted|secondary)\b/.test(cls) && /\brounded-/.test(cls)
				&& /\b(?:size-\d|h-\d|justify-center|inline-flex)\b/.test(cls) && !/\bborder(?:\s|'|"|\{|$|-(?!transparent))/.test(cls);
			if (filled || ghost) {
				hit(file, at, 'primitive', `<${tag}>`, 'hand-made button; use Button from @colophon/ui (its variants own this chrome)');
			}
		}
		if ((tag === 'button' || tag === 'a' || tag === 'span' || tag === 'div')
			&& /\brounded-full\b/.test(cls) && /\bpx-[0-9.]+\b/.test(cls) && /\btext-xs\b/.test(cls)
			&& (/\bbg-[a-z]/.test(cls) || /\bborder\b/.test(cls) || /\{[A-Z_a-z]*(TONE|BADGE|LOOK)/.test(cls))) {
			hit(file, at, 'primitive', `<${tag}>`, 'hand-made badge/pill; use Badge (or StatusBadge for entity state) from @colophon/ui');
		}
	}
}

function regions(file, text) {
	if (extname(file) === '.css') return [{ kind: 'css', text, offset: 0 }];
	if (extname(file) === '.ts') return [{ kind: 'script', text, offset: 0 }];
	// .svelte: <script>…</script> and <style>…</style> carve out; the rest is markup.
	const out = [];
	const re = /<(script|style)\b[^>]*>([\s\S]*?)<\/\1>/gi;
	let at = 0;
	for (let m; (m = re.exec(text)) !== null; ) {
		if (m.index > at) out.push({ kind: 'markup', text: text.slice(at, m.index), offset: at });
		out.push({ kind: m[1].toLowerCase(), text: m[2], offset: m.index + m[0].indexOf(m[2]) });
		at = m.index + m[0].length;
	}
	if (at < text.length) out.push({ kind: 'markup', text: text.slice(at), offset: at });
	return out;
}

/** Mask comments so a literal in a comment can't flag a line. Positions are kept. */
function decomment(text) {
	return text
		.replace(/<!--[\s\S]*?-->/g, (m) => m.replace(/[^\n]/g, ' '))
		.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '));
}

const files = [];
const walk = (dir) => {
	for (const name of readdirSync(dir, { withFileTypes: true })) {
		const p = join(dir, name.name);
		if (name.isDirectory()) walk(p);
		else if (/\.(svelte|ts|css)$/.test(name.name)) files.push(rel(p));
	}
};
for (const dir of SCAN) walk(join(ROOT, dir));

for (const f of files.sort()) {
	if (SKIP.has(f)) continue;
	const text = readFileSync(join(ROOT, f), 'utf8');
	const studio = f.startsWith(STUDIO);
	for (const region of regions(f, text)) {
		const clean = decomment(region.text);
		if (region.kind === 'css') {
			scanCss(f, clean, region.offset, { allowTokenDecls: true, keywords: true });
			scanClassText(f, clean, region.offset);
		} else if (region.kind === 'style') {
			scanCss(f, clean, region.offset, { allowTokenDecls: false, keywords: true });
			scanClassText(f, clean, region.offset);
		} else if (region.kind === 'script') {
			scanClassText(f, clean, region.offset);
		} else {
			for (const m of clean.matchAll(/\s(class|style|fill|stroke|stop-color|flood-color|lighting-color|color)\s*=\s*("[^"]*"|'[^']*'|\{(?:[^{}]|\{[^{}]*\})*\})/gs)) {
				const name = m[1].toLowerCase();
				const at = region.offset + m.index + m[0].indexOf(m[2]);
				if (name === 'class') scanClassText(f, m[2], at);
				else if (name === 'style') scanStyleAttr(f, m[2], at);
				else for (const c of m[2].matchAll(new RegExp(HEX_OR_FN.source, 'g'))) {
					hit(f, at + c.index, 'hex', c[0], `colour literal in ${name}=; use a token`);
				}
			}
			for (const m of clean.matchAll(/\bstyle:([a-zA-Z-]+)\s*=/g)) {
				if (/^(color|background|background-color|border|border-color|box-shadow|text-shadow|border-radius|font-family|fill|stroke|outline|outline-color|caret-color|accent-color|text-decoration-color|opacity)$/.test(m[1])) {
					hit(f, region.offset + m.index, 'style', `style:${m[1]}`, 'a design property set inline; use a class or var(--…) token');
				}
			}
			if (studio && !IS_PRIMITIVE.has(f)) scanPrimitives(f, clean, region.offset);
		}
	}
}

// Findings carry file-absolute offsets; turn them into line:col per file at print time.
const lineCache = new Map();
const where = (f, at) => {
	if (!lineCache.has(f)) lineCache.set(f, readFileSync(join(ROOT, f), 'utf8'));
	const text = lineCache.get(f);
	const before = text.slice(0, at);
	const line = before.split('\n').length;
	return `${line}:${at - before.lastIndexOf('\n')}`;
};

const lineText = (file, at) => {
	const text = lineCache.get(file) ?? readFileSync(join(ROOT, file), 'utf8');
	lineCache.set(file, text);
	const start = text.lastIndexOf('\n', at) + 1;
	const end = text.indexOf('\n', at);
	return text.slice(start, end < 0 ? undefined : end);
};

const usedExceptions = new Set();
const kept = findings.filter((f) => {
	const i = EXCEPTIONS.findIndex((e) => e.rule === f.rule && e.file === f.file && e.match.test(lineText(f.file, f.at)));
	if (i < 0) return true;
	usedExceptions.add(i);
	return false;
});

for (const f of kept.sort((a, b) => a.file.localeCompare(b.file) || a.at - b.at)) {
	console.log(`${f.file}:${where(f.file, f.at)}  ${f.rule.padEnd(12)} ${f.text}\n    ${f.hint}`);
}
console.log('');
for (const i of [...usedExceptions].sort()) {
	const e = EXCEPTIONS[i];
	console.log(`exception ${e.file} (${e.rule}): ${e.why}`);
}
for (const [i, e] of EXCEPTIONS.entries()) if (!usedExceptions.has(i)) console.log(`STALE exception, matches nothing: ${e.file} (${e.rule})`);
console.log('');
console.log(kept.length === 0 ? 'No design literals.' : `${kept.length} design literal(s).`);
process.exit(kept.length === 0 && usedExceptions.size === EXCEPTIONS.length ? 0 : 1);

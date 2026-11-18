// Languages for an entry and its translations (D-087, D-183). The API keeps a language as a
// BCP-47 tag and has no list of a project's languages on this line, so the editor offers the
// common ones by name ("French — fr"), plus any tag already in the entry's group, plus a tag
// typed under "Another language". A tag is checked here exactly as the server checks its shape
// (`check_locale`), so the Translate button is never enabled for one the server would refuse.

/** The languages offered first, by tag. Names come from the browser (Intl.DisplayNames). */
export const COMMON_LANGUAGES = [
	'en',
	'fr',
	'de',
	'es',
	'it',
	'pt',
	'pt-BR',
	'nl',
	'sv',
	'da',
	'nb',
	'fi',
	'pl',
	'cs',
	'ro',
	'hu',
	'el',
	'tr',
	'ru',
	'uk',
	'ar',
	'he',
	'hi',
	'id',
	'vi',
	'th',
	'ja',
	'ko',
	'zh-Hans',
	'zh-Hant'
] as const;

/** A BCP-47-shaped tag as the server accepts it: a language subtag of two or three letters, then
 *  subtags of one to eight letters or digits, at most 35 characters over all. */
export function isLanguageTag(tag: string): boolean {
	if (tag.length === 0 || tag.length > 35) return false;
	return tag.split('-').every((part, index) => (index === 0 ? /^[A-Za-z]{2,3}$/.test(part) : /^[A-Za-z0-9]{1,8}$/.test(part)));
}

/** The same tag as `other`, whatever its case ("pt-br" and "pt-BR"). */
export const sameTag = (tag: string, other: string) => tag.toLowerCase() === other.toLowerCase();

let names: Intl.DisplayNames | null = null;
/** A language's name in the reader's own language ("French", "Portuguese (Brazil)"); the tag
 *  itself when the browser has no name for it. */
export function languageName(tag: string): string {
	try {
		names ??= new Intl.DisplayNames(typeof navigator === 'undefined' ? ['en'] : [...navigator.languages, 'en'], { type: 'language', fallback: 'code' });
		const name = names.of(tag);
		return name === undefined || name === '' ? tag : name;
	} catch {
		return tag;
	}
}

/** "French — fr": the name people know, then the tag the site and the API use. */
export function languageLabel(tag: string): string {
	const name = languageName(tag);
	return name === tag ? tag : `${name} — ${tag}`;
}

/** The language this browser is set to, as one of the common tags when it is one ("en-GB" → "en"),
 *  for a sensible first choice; `''` when none fits. */
export function browserLanguage(): string {
	const wanted = typeof navigator === 'undefined' ? [] : navigator.languages;
	for (const tag of wanted) {
		const exact = COMMON_LANGUAGES.find((common) => sameTag(common, tag));
		if (exact !== undefined) return exact;
		const primary = COMMON_LANGUAGES.find((common) => sameTag(common, tag.split('-')[0] ?? ''));
		if (primary !== undefined) return primary;
	}
	return '';
}

/** The languages a new translation may be in: the common ones and `extra`, without `taken` (the
 *  entry's own and its group's), each once. */
export function translationChoices(taken: string[], extra: string[] = []): string[] {
	const out: string[] = [];
	for (const tag of [...extra, ...COMMON_LANGUAGES]) {
		if (taken.some((held) => sameTag(held, tag)) || out.some((chosen) => sameTag(chosen, tag))) continue;
		out.push(tag);
	}
	return out;
}

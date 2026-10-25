// The viewer's appearance choice: light, dark or the system's, at standard, medium or high
// contrast. Applied as data-theme and data-contrast on <html>, which tokens.css reads.
// Remembered in localStorage as a convenience; everything works without it.

export type ThemeChoice = 'system' | 'light' | 'dark';
export type ContrastChoice = 'system' | 'standard' | 'medium' | 'high';

const THEME_KEY = 'colophon.theme';
const CONTRAST_KEY = 'colophon.contrast';

function read<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
	try {
		const value = localStorage.getItem(key);
		return value !== null && (allowed as readonly string[]).includes(value) ? (value as T) : fallback;
	} catch {
		return fallback;
	}
}

function write(key: string, value: string) {
	try {
		localStorage.setItem(key, value);
	} catch {
		// Storage unavailable (private mode, blocked): the choice lasts for this page only.
	}
}

export function storedTheme(): ThemeChoice {
	return read(THEME_KEY, ['system', 'light', 'dark'] as const, 'system');
}

export function storedContrast(): ContrastChoice {
	return read(CONTRAST_KEY, ['system', 'standard', 'medium', 'high'] as const, 'system');
}

/** Applies a choice to <html> and remembers it. */
export function applyAppearance(theme: ThemeChoice, contrast: ContrastChoice) {
	const root = document.documentElement;
	if (theme === 'system') delete root.dataset['theme'];
	else root.dataset['theme'] = theme;
	const level = contrast === 'system' ? (matchMedia('(prefers-contrast: more)').matches ? 'high' : 'standard') : contrast;
	if (level === 'standard') delete root.dataset['contrast'];
	else root.dataset['contrast'] = level;
	write(THEME_KEY, theme);
	write(CONTRAST_KEY, contrast);
}

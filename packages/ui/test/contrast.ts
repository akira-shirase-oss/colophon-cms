// WCAG 2.x contrast for OKLCH colours, computed from first principles (Björn Ottosson's OKLab
// matrices) rather than by the library that renders them.

export interface Oklch {
	l: number;
	c: number;
	h: number;
}

/** Parses `oklch(L C H)` with L in 0..1 (or a percentage). */
export function parseOklch(text: string): Oklch {
	const match = /^oklch\(\s*([\d.]+)(%?)\s+([\d.]+)\s+([\d.]+)\s*\)$/.exec(text.trim());
	if (match === null) throw new Error(`not an oklch() colour: ${text}`);
	const [, l = '', percent, c = '', h = ''] = match;
	return { l: Number(l) / (percent === '%' ? 100 : 1), c: Number(c), h: Number(h) };
}

/** Linear-light sRGB channels, clamped to the gamut. */
function linearSrgb({ l: L, c, h }: Oklch): [number, number, number] {
	const a = c * Math.cos((h * Math.PI) / 180);
	const b = c * Math.sin((h * Math.PI) / 180);
	const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
	const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
	const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
	const clamp = (x: number) => Math.min(1, Math.max(0, x));
	return [
		clamp(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
		clamp(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
		clamp(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s)
	];
}

export function luminance(colour: Oklch): number {
	const [r, g, b] = linearSrgb(colour);
	return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export function contrast(a: Oklch, b: Oklch): number {
	const [light, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
	return (light + 0.05) / (dark + 0.05);
}

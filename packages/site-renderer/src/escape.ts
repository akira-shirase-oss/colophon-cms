// Escaping, as the content renderer does it (docs/modules/12-content-renderer.md): every string
// the renderer emits is escaped at the seam, so markup can never come from content.

export function escapeText(text: string): string {
	return text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
}

export function escapeAttribute(text: string): string {
	return escapeText(text).replaceAll('"', '&quot;').replaceAll("'", '&#39;');
}

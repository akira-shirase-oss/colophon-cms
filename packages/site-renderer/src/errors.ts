/** Thrown when stored site content can't be rendered — the renderer refuses rather than guess. */
export class UnrenderableSite extends Error {
	override name = 'UnrenderableSite';
}

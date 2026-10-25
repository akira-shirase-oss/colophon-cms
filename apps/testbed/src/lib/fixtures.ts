// The shared rich-text fixtures (fixtures/rich-text/v1), bundled at build time.

export interface RichTextFixture {
	id: string;
	kind: 'valid' | 'invalid';
	name: string;
	description: string;
	value: unknown;
	reject?: { code: string; path: string };
}

const files = import.meta.glob<Omit<RichTextFixture, 'id' | 'kind' | 'name'>>(
	'../../../../fixtures/rich-text/v1/*/*.json',
	{ eager: true, import: 'default' }
);

export const fixtures: RichTextFixture[] = Object.entries(files)
	.map(([path, file]) => {
		const [kind, file_name] = path.split('/').slice(-2) as ['valid' | 'invalid', string];
		const name = file_name.replace(/\.json$/, '');
		return { ...file, kind, name, id: `${kind}/${name}` };
	})
	.sort((a, b) => a.id.localeCompare(b.id));

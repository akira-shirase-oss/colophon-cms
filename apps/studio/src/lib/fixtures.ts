// The shared fixtures (fixtures/rich-text/v1), bundled at build time so the spike can load any
// of them into the editor.

import type { RichTextValue } from '@colophon/editor';

interface FixtureFile {
	description: string;
	value: RichTextValue;
	reject?: { code: string; path: string };
}

export interface Fixture extends FixtureFile {
	id: string;
	kind: 'valid' | 'invalid';
	name: string;
}

const files = import.meta.glob<FixtureFile>('../../../../fixtures/rich-text/v1/*/*.json', {
	eager: true,
	import: 'default'
});

export const fixtures: Fixture[] = Object.entries(files)
	.map(([path, file]) => {
		const [kind, name] = path.split('/').slice(-2) as ['valid' | 'invalid', string];
		return { ...file, kind, name: name.replace(/\.json$/, ''), id: `${kind}/${name.replace(/\.json$/, '')}` };
	})
	.sort((a, b) => a.id.localeCompare(b.id));

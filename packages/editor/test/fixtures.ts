// The shared fixtures (fixtures/rich-text/v1), loaded for the tests.

import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { RichTextValue } from '../src/index.ts';

// A plain path: under happy-dom, `URL` is the DOM's class, which Node's fileURLToPath rejects.
const root = join(import.meta.dirname, '../../../fixtures/rich-text/v1/');

export interface Fixture {
	name: string;
	description: string;
	value: RichTextValue;
	reject?: { code: string; path: string };
}

function load(kind: 'valid' | 'invalid'): Fixture[] {
	return readdirSync(`${root}${kind}`)
		.filter((file) => file.endsWith('.json'))
		.sort()
		.map((file) => ({ name: file.replace(/\.json$/, ''), ...JSON.parse(readFileSync(`${root}${kind}/${file}`, 'utf8')) }));
}

export const valid = load('valid');
export const invalid = load('invalid');

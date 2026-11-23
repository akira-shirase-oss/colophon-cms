// Where sign-in may send a person back to (D-246): a path on this origin only. A browser reads
// `/\host` like `//host`, another host, so a prefix check is not enough; the path is resolved and
// its origin compared.

import { expect, test } from '@playwright/test';
import { sameOrigin } from '../src/lib/api.ts';

test('a path on this origin is a place to come back to', () => {
	for (const path of ['/', '/studio/', '/oauth/authorize?client_id=x&state=y', '/studio/connect/01a2']) {
		expect(sameOrigin(path), path).toBe(true);
	}
});

test('anything that leaves this origin is not', () => {
	for (const path of [
		'//evil.example/',
		'/\\evil.example/',
		'/\\/evil.example',
		'https://evil.example/',
		'javascript:alert(1)',
		'evil.example',
		'/\tevil',
		''
	]) {
		expect(sameOrigin(path), JSON.stringify(path)).toBe(false);
	}
});

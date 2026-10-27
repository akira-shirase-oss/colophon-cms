// `cn` knows the design system's own shadows (D-056), so a component's `shadow-soft` can be
// overridden by a caller's `shadow-none` instead of both landing in the class list.

import { expect, test } from 'vitest';
import { cn } from '../src/lib/utils.ts';
test('custom shadows merge like built-in ones', () => {
	expect(cn('shadow-soft border', 'shadow-none')).toBe('border shadow-none');
	expect(cn('shadow-hairline', 'shadow-float')).toBe('shadow-float');
	expect(cn('px-2', 'px-3')).toBe('px-3');
});

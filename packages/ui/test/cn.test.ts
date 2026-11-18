// `cn` knows the design system's own shadows, shapes and card padding (D-056, D-182), so a
// component's `shadow-soft`, `rounded-card` or `p-card` can be overridden by a caller's class
// instead of both landing in the class list.

import { expect, test } from 'vitest';
import { cn } from '../src/lib/utils.ts';
test('custom shadows merge like built-in ones', () => {
	expect(cn('shadow-soft border', 'shadow-none')).toBe('border shadow-none');
	expect(cn('shadow-hairline', 'shadow-float')).toBe('shadow-float');
	expect(cn('px-2', 'px-3')).toBe('px-3');
});

test('named shapes and card padding merge like built-in ones', () => {
	expect(cn('rounded-card border', 'rounded-xl')).toBe('border rounded-xl');
	expect(cn('rounded-lg', 'rounded-control')).toBe('rounded-control');
	expect(cn('p-card', 'p-4')).toBe('p-4');
	expect(cn('px-card', 'px-card-sm')).toBe('px-card-sm');
	expect(cn('gap-card', 'gap-2')).toBe('gap-2');
});

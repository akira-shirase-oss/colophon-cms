// Allowed forms of text (D-069) in the Studio's words: the server's two refusals said plainly, with
// no internal terms. The pattern itself is never imitated here; only its refusals are worded.

import { expect, test } from '@playwright/test';
import { ApiError } from '../src/lib/api.ts';
import { describe } from '../src/lib/errors.ts';
import { INTERNAL_TERMS, INVALID_PATTERN, patternMismatch } from '../src/lib/words.ts';

test('a value not in its allowed form is said with the field’s name and its help text', () => {
	expect(patternMismatch('Code', 'Like ABC-1234')).toBe("Code isn't in its allowed form. Like ABC-1234");
	expect(patternMismatch('Code')).toBe("Code isn't in its allowed form.");
	expect(patternMismatch('Code', '   ')).toBe("Code isn't in its allowed form.");
});

test('both refusals read plainly, through the shared error words', () => {
	const invalid = new ApiError('refused', '`pattern` is a regular expression of 1 to 500 characters, in Rust regex syntax', 422, 'invalid_pattern', null, {
		reason: 'invalid_pattern',
		pointer: '/fields/1/pattern'
	});
	expect(describe(invalid).text).toBe(INVALID_PATTERN);
	const mismatch = new ApiError('refused', "Code isn't in the form this field asks for", 422, 'pattern_mismatch', null, {
		reason: 'pattern_mismatch',
		pointer: '/code'
	});
	expect(describe(mismatch).text).toBe("Code isn't in the form this field asks for.");
	for (const words of [INVALID_PATTERN, patternMismatch('Code', 'Like ABC-1234'), describe(mismatch).text]) {
		expect(INTERNAL_TERMS.test(words), words).toBe(false);
	}
});

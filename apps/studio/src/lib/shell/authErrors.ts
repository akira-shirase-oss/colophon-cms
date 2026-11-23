/**
 * What a sign-in or sign-up refusal says, and where (D-235): a 422 names its field by JSON
 * Pointer, so its message goes under that field; a 429 says when to try again; anything else is
 * one message for the whole form.
 */
import { ApiError } from '#lib/api.ts';

export type AuthField = 'display_name' | 'email' | 'password' | 'turnstile_token';

export interface AuthRefusal {
	/** Messages to show under a field. */
	fields: Partial<Record<AuthField, string>>;
	/** A message for the form as a whole; empty when every message has a field. */
	form: string;
	/** For a 429: when another attempt may go, in milliseconds since the epoch. */
	retryAt: number | null;
}

const FIELDS: Record<string, AuthField> = {
	'/display_name': 'display_name',
	'/email': 'email',
	'/password': 'password',
	'/turnstile_token': 'turnstile_token'
};

/** "after 14:32", or "in a moment" when it is under a minute away. */
export function retryWords(at: number, now = Date.now(), locale?: string): string {
	if (at - now < 60_000) return 'in a moment';
	const time = new Date(at).toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' });
	return `after ${time}`;
}

/** The Studio's own sentence for a field refusal, by the server's reason; the server's words
 *  (lowercase, for API clients) only when the reason is one it doesn't know. `password` tells a
 *  short `weak_password` from a long one. */
export function fieldWords(reason: string | null, serverMessage: string, password = ''): string {
	switch (reason) {
		case 'weak_password':
			return new TextEncoder().encode(password).length > 512 ? 'Use a password of 512 bytes or fewer.' : 'Use at least 8 characters.';
		case 'invalid_email':
			return 'Enter an email address like name@example.com.';
		case 'bot_check_failed':
			return "The check that you're a person didn't pass. Try it again.";
		default:
			return serverMessage.charAt(0).toUpperCase() + serverMessage.slice(1);
	}
}

export function refusal(failure: unknown, mode: 'signin' | 'signup', now = Date.now(), password = ''): AuthRefusal {
	if (!(failure instanceof ApiError)) return { fields: {}, form: String(failure), retryAt: null };
	if (failure.status === 429) {
		const at = failure.retryAt ?? now + (failure.retryAfter ?? 1000);
		const what = mode === 'signup' ? 'Too many sign-ups from this network' : 'Too many attempts';
		return { fields: {}, form: `${what}. Try again ${retryWords(at, now)}.`, retryAt: at };
	}
	// The server's sign-in failure is generic by design; for a password it means one thing.
	if (mode === 'signin' && failure.status === 400) {
		return { fields: {}, form: 'That email and password did not match.', retryAt: null };
	}
	const field = failure.details?.pointer ? FIELDS[failure.details.pointer] : undefined;
	if (failure.status === 422 && field) {
		return { fields: { [field]: fieldWords(failure.details?.reason ?? failure.reason, failure.message, password) }, form: '', retryAt: null };
	}
	return { fields: {}, form: failure.message.charAt(0).toUpperCase() + failure.message.slice(1), retryAt: null };
}

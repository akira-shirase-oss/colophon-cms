// The setup link (D-061, D-082). While nobody is an administrator, the server prints
// `<studio>#setup=<code>` beside the setup code: a fragment, which browsers never send, so the code
// reaches no server or proxy log. The front door reads it, takes it out of the address bar, and
// keeps it in this tab's session storage, because signing in or making an account goes through the
// identity provider and the fragment doesn't survive that redirect. It is forgotten once used, once
// refused, or once anyone is an administrator. Without storage (a private window that refuses it)
// the code only lives in this page, and the person types it after signing in, as before.

const KEPT = 'colophon-setup-code';

/** The code in a `#setup=…` fragment, or null when there is none. Shape only: the server decides. */
export function setupCodeFrom(hash: string): string | null {
	const match = /^#setup=([0-9A-Za-z-]{1,64})$/.exec(hash);
	return match?.[1]?.toUpperCase() ?? null;
}

/** The code kept from a setup link in this tab, or null. */
export function keptSetupCode(): string | null {
	try {
		return sessionStorage.getItem(KEPT);
	} catch {
		return null;
	}
}

export function keepSetupCode(code: string): void {
	try {
		sessionStorage.setItem(KEPT, code);
	} catch {
		// Not kept: after signing in, the person types the code from the terminal.
	}
}

export function forgetSetupCode(): void {
	try {
		sessionStorage.removeItem(KEPT);
	} catch {
		// Nothing was kept.
	}
}

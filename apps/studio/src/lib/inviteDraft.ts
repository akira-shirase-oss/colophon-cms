// Making a role while inviting someone (D-082): Members' Role select ends with "Make a new role…",
// which goes to Settings → Roles, makes the role, lets you choose what it may do, and comes back to
// the invitation with it chosen. What was typed in "Who is it for?" waits in this tab meanwhile.

/** The Role select's last option: not a role, but the way to make one. */
export const NEW_ROLE = '__new_role__';

const KEY = 'colophon:invite-label';

export function keepInviteLabel(project: string, label: string): void {
	try {
		if (label.trim() === '') sessionStorage.removeItem(`${KEY}:${project}`);
		else sessionStorage.setItem(`${KEY}:${project}`, label);
	} catch {
		// Not kept: the person types it again.
	}
}

export function inviteLabel(project: string): string {
	try {
		return sessionStorage.getItem(`${KEY}:${project}`) ?? '';
	} catch {
		return '';
	}
}

export function forgetInviteLabel(project: string): void {
	try {
		sessionStorage.removeItem(`${KEY}:${project}`);
	} catch {
		// Nothing was kept.
	}
}

/** Where the invitation is, with `role` chosen. */
export const backToInvite = (members: string, role: string) => `${members}?role=${encodeURIComponent(role)}#invite`;

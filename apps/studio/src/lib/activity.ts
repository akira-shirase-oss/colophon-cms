// The activity log (step R.4's audit log) in sentences (D-059): "Alex changed Sam's role from
// Editor to Desk". An event names people by identity and roles by key; the page passes what it
// knows to name them, and anything it can't name is said honestly ("someone who has left").

import type { AuditEvent, ContentType, Grant } from './api.ts';
import { grantWords } from './permissions.ts';

/** The kinds of change, in the order the filter offers them, with the words for each. */
export const KINDS: { action: string; label: string }[] = [
	{ action: 'member.role_changed', label: 'Someone’s role changed' },
	{ action: 'member.removed', label: 'Someone removed' },
	{ action: 'member.left', label: 'Someone left' },
	{ action: 'invitation.created', label: 'Invitation made' },
	{ action: 'invitation.revoked', label: 'Invitation revoked' },
	{ action: 'invitation.accepted', label: 'Invitation accepted' },
	{ action: 'role.created', label: 'Role made' },
	{ action: 'role.updated', label: 'Role changed' },
	{ action: 'role.deleted', label: 'Role deleted' },
	{ action: 'token.created', label: 'API token made' },
	{ action: 'token.revoked', label: 'API token revoked' }
];

/** What a page knows to name things with. */
export interface Names {
	/** People by identity id: members now, and every actor on the events loaded. */
	person: (id: string) => string | undefined;
	/** A role's name by its key, as it is now or was when last recorded. */
	role: (key: string) => string;
	/** An invitation's label by its id, from its "made" event if loaded. */
	invitation: (id: string) => string | undefined;
	types: Pick<ContentType, 'slug' | 'definition'>[];
}

const text = (value: unknown): string | undefined => (typeof value === 'string' && value !== '' ? value : undefined);
const quoted = (value: string) => `“${value}”`;
const possessive = (name: string) => `${name}’s`;

/** Who did it: a person's name, or an API token's. */
function actor(event: AuditEvent): string {
	if (event.actor.kind === 'token') return `The API token ${quoted(event.actor.name ?? 'without a name')}`;
	return event.actor.name ?? 'Someone';
}

/** What changed in a role's permissions, as "now may …; no longer …". */
function permissionChange(before: Grant[], after: Grant[], names: Names): string[] {
	const id = (grant: Grant) => JSON.stringify([grant.action, grant.subject, grant.fields ?? null, grant.condition ?? null]);
	const was = new Set(before.map(id));
	const is = new Set(after.map(id));
	const added = after.filter((grant) => !was.has(id(grant))).map((grant) => grantWords(grant, names.types));
	const removed = before.filter((grant) => !is.has(id(grant))).map((grant) => grantWords(grant, names.types));
	return [...(added.length > 0 ? [`now may ${added.join(', ')}`] : []), ...(removed.length > 0 ? [`no longer may ${removed.join(', ')}`] : [])];
}

const grants = (value: unknown): Grant[] => (Array.isArray(value) ? (value as Grant[]) : []);

/** One event as a sentence, ending with a full stop. */
export function sentence(event: AuditEvent, names: Names): string {
	const who = actor(event);
	const detail = event.detail;
	const role = (key: unknown) => (typeof key === 'string' ? names.role(key) : 'a role');
	const person = (id: string) => names.person(id) ?? 'someone who has since left';
	switch (event.action) {
		case 'member.role_changed':
			return `${who} changed ${possessive(person(event.subject))} role from ${role(detail['from'])} to ${role(detail['to'])}.`;
		case 'member.removed':
			return `${who} removed ${person(event.subject)} (${role(detail['role'])}) from the project.`;
		case 'member.left':
			return `${who} left the project (was ${role(detail['role'])}).`;
		case 'invitation.created':
			return `${who} invited ${quoted(text(detail['label']) ?? 'someone')} as ${role(detail['role'])}.`;
		case 'invitation.revoked': {
			const label = names.invitation(event.subject);
			return `${who} revoked the invitation ${label === undefined ? 'link' : `for ${quoted(label)}`}.`;
		}
		case 'invitation.accepted':
			return `${who} joined as ${role(detail['role'])}, with an invitation.`;
		case 'role.created':
			return `${who} made the role ${text(detail['name']) ?? role(event.subject)}.`;
		case 'role.updated': {
			const before = (detail['before'] ?? {}) as Record<string, unknown>;
			const after = (detail['after'] ?? {}) as Record<string, unknown>;
			const parts: string[] = [];
			if (text(before['name']) !== text(after['name'])) parts.push(`renamed it ${text(after['name']) ?? ''}`);
			if ((before['description'] ?? '') !== (after['description'] ?? '')) parts.push('changed what it’s for');
			parts.push(...permissionChange(grants(before['permissions']), grants(after['permissions']), names));
			const named = text(before['name']) ?? role(event.subject);
			return parts.length === 0 ? `${who} saved the role ${named}, unchanged.` : `${who} changed the role ${named}: ${parts.join('; ')}.`;
		}
		case 'role.deleted':
			return `${who} deleted the role ${text(detail['name']) ?? role(event.subject)}.`;
		case 'token.created': {
			const kind = detail['kind'] === 'read-only' ? 'read only' : detail['kind'] === 'full-access' ? 'full access' : 'custom';
			const until = text(detail['expires_at']);
			const end = until === undefined ? '' : `, until ${new Date(until).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' })}`;
			return `${who} made the API token ${quoted(text(detail['name']) ?? 'without a name')} (${kind}${end}).`;
		}
		case 'token.revoked':
			return `${who} revoked the API token ${quoted(text(detail['name']) ?? 'without a name')}.`;
		default:
			return `${who} made a change this Studio can't describe yet.`;
	}
}

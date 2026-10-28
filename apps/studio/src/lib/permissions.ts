// What each permission is called on screen (D-058, D-059): the role editor's grid, a custom API
// token's, and the activity log's sentences use the same words.

import type { Action, ContentType, Grant } from './api.ts';

/** Work on content, in the order an article goes through it, each with one line under its
 *  column where the word alone could mislead (D-082), and the verb a sentence uses ("change the
 *  fields of Articles"). Deleting has no button yet, so it isn't offered; a role that has it keeps it. */
export const WORK: { action: Action; label: string; hint?: string; verb: string }[] = [
	{ action: 'content.read', label: 'See', hint: 'read drafts', verb: 'see' },
	{ action: 'content.create', label: 'Create', verb: 'create' },
	{ action: 'content.update', label: 'Edit', verb: 'edit' },
	{ action: 'content.review', label: 'Approve', hint: 'OK someone’s publish request', verb: 'approve' },
	{ action: 'content.publish', label: 'Publish', hint: 'put it live', verb: 'publish' },
	{ action: 'schema.change', label: 'Change its fields', hint: 'Add, rename or remove this type’s fields (developers)', verb: 'change the fields of' }
];

/** Field labels for code names, in a content type or, for `*`, in any (D-082): what a limited
 *  permission says ("only Name and Price"). A name no type has stays as it is. */
export function fieldLabels(ids: string[], subject: string, types: Pick<ContentType, 'slug' | 'definition'>[]): string[] {
	const where = subject === '*' ? types : types.filter((type) => type.slug === subject);
	return ids.map((id) => {
		const labels = [...new Set(where.flatMap((type) => type.definition.fields.filter((field) => field.id === id).map((field) => field.label)))];
		return labels.length === 0 ? id : labels.join(' / ');
	});
}

/** Powers over the whole project, which aren't about one content type. */
export const POWERS: { action: Action; label: string; hint: string }[] = [
	{ action: 'members.manage', label: 'Invite people and change their roles', hint: 'Only to roles whose every power this role has.' },
	{ action: 'roles.manage', label: 'Make and change roles', hint: 'Again, never beyond this role’s own powers.' },
	{ action: 'destinations.manage', label: 'Manage where releases go', hint: 'Webhooks, CDNs and checks; their addresses may carry secrets.' },
	{
		action: 'tokens.manage',
		label: 'Manage API tokens',
		hint: 'Make and revoke keys that let scripts and sites use the API, never with more powers than this role.'
	},
	{ action: 'audit.read', label: 'See the activity log', hint: 'Who changed roles, members, invitations and API tokens, and when.' }
];

/** A grid cell's key: an action on a subject (`*` or a content type). */
export const cell = (action: string, subject: string) => `${action}|${subject}`;

/** One permission in words: "Edit Articles", "Publish everything", "See the activity log". */
export function grantWords(grant: Grant, types: Pick<ContentType, 'slug' | 'definition'>[]): string {
	const power = POWERS.find((item) => item.action === grant.action);
	if (power !== undefined) return power.label.slice(0, 1).toLowerCase() + power.label.slice(1);
	const work = WORK.find((item) => item.action === grant.action)?.verb ?? (grant.action === 'content.delete' ? 'delete' : grant.action);
	const where = grant.subject === '*' ? 'everything' : (types.find((type) => type.slug === grant.subject)?.definition.name ?? grant.subject);
	const fields = grant.fields ? `, only ${fieldLabels(grant.fields, grant.subject, types).join(', ')}` : '';
	const own = grant.condition === 'own' ? ', only their own entries' : '';
	return `${work} ${where}${fields}${own}`;
}

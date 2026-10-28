// What each permission is called on screen (D-058, D-059): the role editor's grid, a custom API
// token's, and the activity log's sentences use the same words.

import type { Action, ContentType, Grant } from './api.ts';

/** Work on content, in the order an article goes through it. Deleting has no button yet, so it
 *  isn't offered; a role that has it keeps it. */
export const WORK: { action: Action; label: string }[] = [
	{ action: 'content.read', label: 'See' },
	{ action: 'content.create', label: 'Create' },
	{ action: 'content.update', label: 'Edit' },
	{ action: 'content.review', label: 'Approve' },
	{ action: 'content.publish', label: 'Publish' },
	{ action: 'schema.change', label: 'Change fields' }
];

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
	const work = WORK.find((item) => item.action === grant.action)?.label ?? (grant.action === 'content.delete' ? 'Delete' : grant.action);
	const where = grant.subject === '*' ? 'everything' : (types.find((type) => type.slug === grant.subject)?.definition.name ?? grant.subject);
	const fields = grant.fields ? `, only ${grant.fields.join(', ')}` : '';
	const own = grant.condition === 'own' ? ', only their own entries' : '';
	return `${work.toLowerCase()} ${where}${fields}${own}`;
}

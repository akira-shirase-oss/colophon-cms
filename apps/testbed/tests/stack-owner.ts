// Whether the local stack is this run's own, as scripts/stack-guard.sh decides (D-187): CI, a
// COMPOSE_PROJECT_NAME other than the shared `colophon`, or COLOPHON_STACK_OWNER=1 from the
// stack's owner. A spec that stops, pauses or reconfigures a stack service asks first; on a
// shared stack that work is CI's. scripts/test-stack-guard.sh checks both answer alike.

export function stackIsOurs(env: Record<string, string | undefined> = process.env): boolean {
	if ((env['CI'] ?? '') !== '') return true;
	const project = env['COMPOSE_PROJECT_NAME'] ?? '';
	if (project !== '' && project !== 'colophon') return true;
	return env['COLOPHON_STACK_OWNER'] === '1';
}

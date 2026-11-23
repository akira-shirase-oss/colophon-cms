// Whether the local stack is this run's own, as scripts/stack-guard.sh decides (D-187, D-187b,
// D-187c): exact matches only — CI=true on a GitHub runner (GITHUB_ACTIONS=true and RUNNER_NAME
// set), or a COMPOSE_PROJECT_NAME other than the shared `colophon`. A spec that stops, pauses or
// reconfigures a stack service asks first; on a shared stack that work is CI's.
// scripts/test-stack-guard.sh checks both answer alike.

export function stackIsOurs(env: Record<string, string | undefined> = process.env): boolean {
	if (env['CI'] === 'true' && env['GITHUB_ACTIONS'] === 'true' && (env['RUNNER_NAME'] ?? '') !== '') {
		return true;
	}
	const project = env['COMPOSE_PROJECT_NAME'] ?? '';
	return project !== '' && project !== 'colophon';
}

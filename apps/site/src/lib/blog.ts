// How the blog names the build log's milestones and kinds of post. The build log's "D" is the
// design work before any code (day 0), not the roadmap's later developer milestone.
export const MILESTONE_NAMES: Record<string, string> = {
	D: 'Design, before any code',
	P0: 'Foundation',
	P1: 'Never lose an edit',
	P2: 'Publish exactly once',
	P3: 'Prove it reached the site',
	P4: 'Time travel and media',
	P5: 'Safe automation',
	P6: 'Operate it'
};

export const KIND = {
	release: { word: 'Release', variant: 'brand' },
	experiment: { word: 'Experiment', variant: 'outline' },
	failure: { word: 'Failure', variant: 'attention' }
} as const;

const DATE = new Intl.DateTimeFormat('en-GB', { day: 'numeric', month: 'long', year: 'numeric', timeZone: 'UTC' });

/** "26 October 2026", in UTC so the build machine's zone doesn't move a date. */
export function longDate(iso: string): string {
	return DATE.format(new Date(iso));
}

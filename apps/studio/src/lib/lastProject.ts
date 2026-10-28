// The project someone last opened in this browser, so the home page shows their work there first.
// Kept per person, in this browser only; storage can be missing or refuse (private windows), and
// then the home page starts with their first project.

const KEY = 'colophon:last-project';

export function rememberProject(identity: string, project: string): void {
	try {
		localStorage.setItem(`${KEY}:${identity}`, project);
	} catch {
		// Not kept: the home page falls back to the first project.
	}
}

export function lastProject(identity: string): string | null {
	try {
		return localStorage.getItem(`${KEY}:${identity}`);
	} catch {
		return null;
	}
}

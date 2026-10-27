// Shapes shared by server loads and components.

/** A page the navigation lists. */
export interface DocLink {
	href: string;
	title: string;
}

/** A design document as the navigation lists it (lib/server/repository-docs.ts makes them). */
export interface DesignDocLink extends DocLink {
	group: 'Project' | 'Modules' | 'Formats' | 'Brand';
	source: string;
}

/** The pages made from the repository's own files: release notes, upgrade guides, design documents. */
export interface RepoLinks {
	releases: DocLink[];
	upgrades: DocLink[];
	design: DesignDocLink[];
}

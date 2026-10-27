// Shapes shared by server loads and components.

/** A design document as the navigation lists it (lib/server/design-docs.ts makes them). */
export interface DesignDocLink {
	href: string;
	title: string;
	group: 'Project' | 'Modules' | 'Formats' | 'Brand';
	source: string;
}

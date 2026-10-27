// See https://svelte.dev/docs/kit/types#app.d.ts
declare global {
	/** The short commit hash the site was built from (vite.config.ts). */
	const __COMMIT__: string;
	namespace App {
		// interface Error {}
		// interface Locals {}
		// interface PageData {}
		// interface PageState {}
		// interface Platform {}
	}
}

export {};

import { defineConfig } from 'vitest/config';

export default defineConfig({
	test: {
		// The safety tests parse rendered HTML back into a DOM; happy-dom provides one. The real-
		// browser checks run in the Studio's Playwright suite.
		environment: 'happy-dom',
		include: ['test/**/*.test.ts']
	}
});

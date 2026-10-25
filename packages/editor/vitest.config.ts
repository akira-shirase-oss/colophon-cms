import { defineConfig } from 'vitest/config';

export default defineConfig({
	test: {
		// Tiptap needs a DOM; happy-dom is a fast stand-in. The real-browser checks run in the
		// Studio's Playwright suite.
		environment: 'happy-dom',
		include: ['test/**/*.test.ts']
	}
});

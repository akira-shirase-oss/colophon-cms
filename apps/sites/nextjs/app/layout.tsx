import type { Metadata } from 'next';
import type { ReactNode } from 'react';
import { buildRelease } from '../lib/colophon.ts';

// The release this page was built from, as <meta name="colophon-generation">: a Colophon site check reads it to
// verify a release reached the site.
export function generateMetadata(): Metadata {
	const pinned = buildRelease();
	return {
		title: 'Butter & Thyme',
		...(pinned ? { other: { 'colophon-generation': String(pinned.generation) } } : {})
	};
}

export default function RootLayout({ children }: { children: ReactNode }) {
	const pinned = buildRelease();
	return (
		<html lang="en">
			<body style={{ fontFamily: 'system-ui, sans-serif', maxWidth: '42rem', margin: '2rem auto', padding: '0 1rem', lineHeight: 1.5 }}>
				<header style={{ marginBottom: '2rem' }}>
					<a href="/">Butter &amp; Thyme</a> <small>a Next.js demo of Colophon</small>
				</header>
				<main>{children}</main>
				<footer style={{ marginTop: '3rem', color: '#555', fontSize: '0.875rem' }}>
					{pinned ? `Built from release ${pinned.generation}.` : null}
				</footer>
			</body>
		</html>
	);
}

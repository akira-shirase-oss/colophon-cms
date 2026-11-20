// A fully static export (D-220): every page is built once, at the release scripts/pin-release.mjs pinned.
/** @type {import('next').NextConfig} */
export default {
	output: 'export',
	trailingSlash: true,
	images: { unoptimized: true }
};

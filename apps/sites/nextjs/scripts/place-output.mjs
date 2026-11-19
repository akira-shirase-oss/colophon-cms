// After `next build`: the static export is in out/; move it where the demo sites keep their builds, dist/ (or
// SITE_OUT_DIR, which scripts/serve-demo-sites.sh sets to dist.next before swapping it in).
import { renameSync, rmSync } from 'node:fs';

const target = process.env.SITE_OUT_DIR ?? 'dist';
rmSync(target, { recursive: true, force: true });
renameSync('out', target);
console.log(`place-output: the site is in ${target}/`);

// Regenerates `src/api.types.ts` from the committed OpenAPI artifact
// (`docs/api/openapi.json`, DX.7/D-110), mirroring that artifact's own no-drift rule: `npm run
// gen` rewrites the file and `npm run check` fails when the committed types differ from what
// the document produces today. Both go through openapi-typescript's programmatic API, so the
// check never writes.
//
//   node scripts/gen-types.mjs           # rewrite src/api.types.ts
//   node scripts/gen-types.mjs --check   # fail (exit 1) when src/api.types.ts is stale

import { readFileSync, writeFileSync } from 'node:fs';
import openapiTS, { astToString, COMMENT_HEADER } from 'openapi-typescript';

const DOCUMENT = new URL('../../../docs/api/openapi.json', import.meta.url);
const OUTPUT = new URL('../src/api.types.ts', import.meta.url);

const ast = await openapiTS(DOCUMENT, {
	// The document's user-defined shapes (EntryDocument, QueryDocument, TypeDefinition, rich
	// text) are bare `type: object` schemas: `Record<string, unknown>`, not the unusable
	// `Record<string, never>` the default gives.
	emptyObjectsUnknown: true
});
const generated = COMMENT_HEADER + astToString(ast);

if (process.argv.includes('--check')) {
	const committed = readFileSync(OUTPUT, 'utf8');
	if (committed !== generated) {
		console.error(
			'src/api.types.ts is stale: regenerate it with `npm run gen -w @colophon-cms/sdk` ' +
				'(it is built from docs/api/openapi.json, which `COLOPHON_UPDATE_OPENAPI=1 ' +
				'cargo test -p colophon-api --lib openapi` refreshes).'
		);
		process.exit(1);
	}
	console.log('src/api.types.ts is current.');
} else {
	writeFileSync(OUTPUT, generated);
	console.log('wrote src/api.types.ts');
}

// Writes src/tokens.css from theme.json. Run: npm run generate -w @colophon/ui
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tokensCss, type Theme } from './tokens.ts';

const root = join(import.meta.dirname, '..');
const theme = JSON.parse(readFileSync(join(root, 'theme.json'), 'utf8')) as Theme;
writeFileSync(join(root, 'src/tokens.css'), tokensCss(theme));
console.log(`wrote src/tokens.css from seed ${theme.seed}`);

// Every Idempotency-Key a k6 scenario sends comes from a builder in perf/k6/lib (idempotencyKey, raceKeys,
// sequenceKey) or a random uuid(), never a template written by hand. The header's value may be a name: then
// that name's definition in the file is checked, and a template literal left in it once the builder calls are
// removed fails. undo replayed a hand-derived `${key}-u1` while sending a built key (an unseen key, never a
// replay), and bulk's `${prefix}-${ms}-${seq}` collided across VUs (CI 44698136206). Prints each failure.
//
//   node scripts/check-idempotency-keys.mjs perf/k6/*.js
import { readFileSync } from 'node:fs';

const BUILDERS = /\b(idempotencyKey|raceKeys|sequenceKey)\(/g;
// The text with every builder call's argument list removed (parentheses balanced, templates skipped).
function withoutBuilders(text) {
  let out = '', i = 0;
  for (const m of text.matchAll(BUILDERS)) {
    if (m.index < i) continue;
    out += text.slice(i, m.index);
    let depth = 0, j = m.index + m[0].length - 1;
    for (; j < text.length; j += 1) {
      if (text[j] === '(') depth += 1;
      else if (text[j] === ')' && (depth -= 1) === 0) break;
    }
    i = j + 1;
  }
  return out + text.slice(i);
}
// A name's definitions: `const|let|var NAME = …;` to its line's end, or `function NAME(…) { … }` to the next
// line that is only `}`. A name with none (a parameter) has nothing to check here.
function definitions(src, name) {
  const defs = [];
  const lines = src.split('\n');
  lines.forEach((line, n) => {
    if (new RegExp(`\\b(const|let|var)\\s+${name}\\s*=`).test(line)) defs.push({ n: n + 1, text: line });
    if (new RegExp(`^\\s*function\\s+${name}\\s*\\(`).test(line)) {
      let end = n;
      while (end < lines.length - 1 && !/^\s*}\s*$/.test(lines[end])) end += 1;
      defs.push({ n: n + 1, text: lines.slice(n, end + 1).join('\n') });
    }
  });
  return defs;
}
let failures = 0;
for (const file of process.argv.slice(2)) {
  const src = readFileSync(file, 'utf8');
  const names = new Set([...src.matchAll(/'Idempotency-Key'\s*:\s*([A-Za-z_$][\w$]*)/g)].map((m) => m[1]));
  for (const name of names) {
    if (['idempotencyKey', 'raceKeys', 'sequenceKey', 'uuid'].includes(name)) continue;
    for (const def of definitions(src, name)) {
      if (withoutBuilders(def.text).includes('`')) {
        console.log(`${file}:${def.n}: Idempotency-Key ${name} is built by hand: ${def.text.trim().split('\n')[0]}`);
        failures += 1;
      }
    }
  }
}
process.exit(failures ? 1 : 0);

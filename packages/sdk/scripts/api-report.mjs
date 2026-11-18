#!/usr/bin/env node
// The public-API report (D-170): every export of a package's emitted `dist/index.d.ts`, one line
// each, with the signatures of what is written by hand — classes (constructor, public methods
// and properties), functions, constants, and interfaces (their members). Types generated from
// the OpenAPI document are listed by name only: their shape is reviewed in docs/api/openapi.json.
// The report is committed beside each package (`api-report.txt`); the pack check compares the
// packed package's report with it, so a removed, renamed or re-typed export shows up in review
// instead of in a user's build.
//
//   node packages/sdk/scripts/api-report.mjs <dist/index.d.ts>            # prints the report
//   node packages/sdk/scripts/api-report.mjs <dist/index.d.ts> <file>     # fails if <file> differs
//   node packages/sdk/scripts/api-report.mjs <dist/index.d.ts> <file> --update
import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';

const require = createRequire(import.meta.url);
const ts = require('typescript');

export function report(entry) {
	const program = ts.createProgram([entry], {
		target: ts.ScriptTarget.ES2023,
		module: ts.ModuleKind.NodeNext,
		moduleResolution: ts.ModuleResolutionKind.NodeNext,
		lib: ['lib.es2023.d.ts', 'lib.dom.d.ts'],
		strict: true,
		noEmit: true,
		skipLibCheck: true,
		types: []
	});
	const checker = program.getTypeChecker();
	const source = program.getSourceFile(entry);
	if (source === undefined) throw new Error(`can't read ${entry}`);
	// Declarations as written in the .d.ts, comments dropped and whitespace folded: what a caller
	// compiles against, without expanding the generated wire types into every signature.
	const printer = ts.createPrinter({ removeComments: true });
	const text = (node) => printer.printNode(ts.EmitHint.Unspecified, node, node.getSourceFile()).replace(/\s+/g, ' ').trim();
	const generated = (declaration) => /api\.types\.d\.ts$/.test(declaration.getSourceFile().fileName);
	const lines = [];
	for (const exported of checker.getExportsOfModule(checker.getSymbolAtLocation(source))) {
		const symbol = exported.flags & ts.SymbolFlags.Alias ? checker.getAliasedSymbol(exported) : exported;
		const name = exported.getName();
		const declarations = symbol.declarations ?? [];
		if (declarations.length === 0) {
			lines.push(`unknown ${name}`);
		} else if (declarations.every(generated)) {
			lines.push(`${name}: generated from the OpenAPI document`);
		} else {
			for (const declaration of declarations) {
				if (ts.isClassDeclaration(declaration) || ts.isInterfaceDeclaration(declaration)) {
					const keyword = ts.isClassDeclaration(declaration) ? 'class' : 'interface';
					const heritage = (declaration.heritageClauses ?? []).map(text).join(' ');
					lines.push(`${keyword} ${name}${declaration.typeParameters ? `<${declaration.typeParameters.map(text).join(', ')}>` : ''}${heritage ? ` ${heritage}` : ''}`);
					for (const member of declaration.members) {
						if (member.name !== undefined && ts.isPrivateIdentifier(member.name)) continue;
						const modifiers = ts.canHaveModifiers(member) ? (ts.getModifiers(member) ?? []) : [];
						if (modifiers.some((m) => m.kind === ts.SyntaxKind.PrivateKeyword || m.kind === ts.SyntaxKind.ProtectedKeyword)) continue;
						lines.push(`  ${text(member)}`);
					}
				} else if (ts.isVariableDeclaration(declaration)) {
					lines.push(`const ${text(declaration)}`);
				} else {
					lines.push(text(declaration).replace(/^export (declare )?/, ''));
				}
			}
		}
	}
	return `${lines.join('\n')}\n`;
}

if (import.meta.url === `file://${process.argv[1]}`) {
	const [entry, file, mode] = process.argv.slice(2);
	if (entry === undefined) {
		console.error('usage: api-report.mjs <dist/index.d.ts> [<report file> [--update]]');
		process.exit(2);
	}
	const text = report(path.resolve(entry));
	if (file === undefined) {
		process.stdout.write(text);
	} else if (mode === '--update') {
		writeFileSync(file, text);
	} else if (readFileSync(file, 'utf8') !== text) {
		console.error(`${file} is not the public API this build exports. If the change is intended, run:\n  node packages/sdk/scripts/api-report.mjs ${entry} ${file} --update`);
		process.exit(1);
	}
}

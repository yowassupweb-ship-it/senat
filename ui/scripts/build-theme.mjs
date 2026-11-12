import fs from 'node:fs';
const tokens = JSON.parse(fs.readFileSync(new URL('../tokens/theme.json', import.meta.url), 'utf8'));
const declarations = theme => Object.entries(theme).map(([key, token]) => `  --${key}: ${token.$value};`).join('\n');
const light = declarations(tokens.light);
const dark = declarations(tokens.dark);
fs.writeFileSync(new URL('../src/theme.css', import.meta.url), `/* Generated from tokens/theme.json. */\n:root {\n${light}\n  color-scheme: light;\n}\n@media (prefers-color-scheme: dark) {\n  :root:not([data-theme='light']) {\n${dark}\n  color-scheme: dark;\n  }\n}\n:root[data-theme='dark'] {\n${dark}\n  color-scheme: dark;\n}\n`);

import fs from 'node:fs';
const report = JSON.parse(fs.readFileSync(new URL('../test-results/results.json', import.meta.url), 'utf8'));
const { expected, unexpected, skipped, flaky } = report.stats;
const total = expected + unexpected + skipped + flaky;
console.log(`${expected}/${total} browser tests passed; ${unexpected} failed; ${skipped} skipped; ${flaky} flaky.`);
console.log(`Run: ${report.stats.startTime}`);
if (unexpected || skipped || flaky || !total) process.exitCode = 1;

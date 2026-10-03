import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';

// Capture the full scope before changing any completion status. This script
// never infers that a requirement passed from filenames, tests or model claims.
const path = 'docs/CONDUCTOR_MASTER_BUILD_PROMPT.md';
const specification = readFileSync(path, 'utf8');
const lines = specification.split(/\r?\n/);
const sections = [];
let section;
for (const [index, raw] of lines.entries()) {
  const heading = /^# (\d+)\. (.+)/.exec(raw);
  if (heading) {
    section = { number: Number(heading[1]), title: heading[2], line: index + 1, status: 'unverified', evidence: [], requirements: [] };
    sections.push(section);
  } else if (section && raw.trim() && raw.trim() !== '---') {
    section.requirements.push({ id: `${section.number}.${section.requirements.length + 1}`, line: index + 1, text: raw, status: 'unverified', evidence: [] });
  }
}
if (sections.length !== 167 || sections.some((s, i) => s.number !== i + 1)) {
  throw new Error('Expected all 167 ordered specification sections; audit scope must not shrink.');
}
const result = {
  schema: 1,
  source: path,
  source_sha256: createHash('sha256').update(specification).digest('hex'),
  policy: 'Unverified is not complete. Each requirement needs direct authoritative evidence, including its full scope. The raw master specification remains authoritative.',
  sections,
};
const destination = 'docs/requirements.json';
try {
  const previous = JSON.parse(readFileSync(destination, 'utf8'));
  if (previous.source_sha256 !== result.source_sha256) throw new Error('Specification changed. Review migration of requirement IDs before regeneration.');
  // Preserve reviewed statuses and evidence. Never silently reset an audit.
  for (const next of sections) {
    const old = previous.sections.find((item) => item.number === next.number);
    if (!old) continue;
    next.status = old.status;
    next.evidence = old.evidence ?? [];
    for (const requirement of next.requirements) {
      const original = old.requirements.find((item) => item.id === requirement.id && item.text === requirement.text);
      if (original) { requirement.status = original.status; requirement.evidence = original.evidence; }
    }
  }
} catch (error) {
  if (error.code !== 'ENOENT') throw error;
}
writeFileSync(destination, `${JSON.stringify(result, null, 2)}\n`);
writeFileSync('docs/SPEC_CHECKLIST.md', [
  '# Full specification checklist',
  '',
  'All 167 sections remain in scope. The machine-readable [requirements registry](requirements.json) retains every nonblank source line with its source line number, status and evidence. No feature is complete merely because code exists.',
  '',
  '| Section | Requirement area | Current audit status |',
  '| --- | --- | --- |',
  ...sections.map((item) => `| ${item.number} | [${item.title.replace(/\|/g, '\\|')}](CONDUCTOR_MASTER_BUILD_PROMPT.md) | ${item.status} |`),
  '',
  'See [implementation audit](IMPLEMENTATION_AUDIT.md) for acceptance gates and [verification record](VERIFICATION.md) for current passing evidence. Update reviewed status/evidence in requirements.json; rerun this script to refresh the checklist without losing evidence.',
  '',
].join('\n'));
console.log(`Captured ${sections.length} sections and ${sections.reduce((n, s) => n + s.requirements.length, 0)} source requirement lines.`);

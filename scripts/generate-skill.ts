#!/usr/bin/env bun
import { mkdir, rm, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

type Reference = {
  source: string;
  target: string;
  title: string;
  description: string;
};

const scriptDir = dirname(fileURLToPath(import.meta.url));
const rootDir = join(scriptDir, "..");
const skillDir = join(rootDir, "skills", "crabenv");
const referencesDir = join(skillDir, "references");

const references: Reference[] = [
  {
    source: "docs/language/typescript.md",
    target: "typescript-javascript.md",
    title: "TypeScript / JavaScript",
    description: "Schema conventions for @t3-oss/env-core, zod, public/private env files, and monorepo with-env scripts.",
  },
  {
    source: "docs/language/python.md",
    target: "python.md",
    title: "Python",
    description: "Pydantic settings conventions for env.py, Field aliases, and monorepo run scripts.",
  },
  {
    source: "docs/language/rust.md",
    target: "rust.md",
    title: "Rust",
    description: "Serde/Figment conventions for src/config.rs, explicit env renames, and workspace run scripts.",
  },
  {
    source: "docs/language/flutter.md",
    target: "flutter-dart.md",
    title: "Flutter / Dart",
    description: "String.fromEnvironment conventions, dart-define files, and public-only mobile env rules.",
  },
  {
    source: "docs/sinks/index.md",
    target: "github-actions.md",
    title: "GitHub Actions Sinks",
    description: "Deployment sink notes for GitHub Actions.",
  },
];

async function readSource(path: string) {
  const file = Bun.file(join(rootDir, path));
  if (!(await file.exists())) {
    throw new Error(`Missing source doc: ${path}`);
  }
  return file.text();
}

function stripFrontmatter(markdown: string) {
  return markdown.replace(/^---\n[\s\S]*?\n---\n+/, "");
}

function demoteHeadings(markdown: string) {
  return markdown.replace(/^#/gm, "##");
}

function skillContent(indexBody: string, cliGuideBody: string) {
  return `---
name: crabenv
description: Understand and apply the crabenv env var management standard: one local env, aligned schemas, templates, docs, and deployment sinks across languages.
---

${stripFrontmatter(indexBody).trim()}

${demoteHeadings(stripFrontmatter(cliGuideBody)).trim()}

## Skill references

Use the concept above first. For exact language examples, read the matching reference file:

${references.map((reference) => `- \`references/${reference.target}\` — ${reference.description}`).join("\n")}
`;
}

function referenceContent(reference: Reference, sourceBody: string) {
  return `# ${reference.title}

> ${reference.description}

${stripFrontmatter(sourceBody).trim()}
`;
}

await rm(skillDir, { recursive: true, force: true });
await mkdir(referencesDir, { recursive: true });

const indexBody = await readSource("docs/index.md");
const cliGuideBody = await readSource("docs/cli-guide.md");
await writeFile(join(skillDir, "SKILL.md"), skillContent(indexBody, cliGuideBody));

for (const reference of references) {
  const body = await readSource(reference.source);
  await writeFile(join(referencesDir, reference.target), referenceContent(reference, body));
}

console.log(`Generated ${skillDir}`);

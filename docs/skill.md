---
title: Agent Skills
description: Install the crabenv agent skill when you want an AI coding agent
---

Install the crabenv agent skill when you want an AI coding agent to apply the crabenv standard without needing the CLI installed in the target project.

```sh
crabenv skill # thin wrapper around npx skills (recommended when crabenv is installed)
crabenv skill --global # user-level install instead of project-level
crabenv skill --dry-run # print the underlying npx command without running it
```

Or without crabenv:

```sh
npx skills add blankeos/crabenv
```

The skill includes language-specific references for TypeScript/JavaScript, Python, Rust, and Flutter/Dart, plus core monorepo rules and deployment sink guidance.

Agents should use the skill to audit or create `.env`, `.env.example`, schema/config files, and explicit sink wiring while keeping those surfaces aligned.

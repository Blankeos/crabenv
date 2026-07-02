---
title: CLI Guide
description: Compact crabenv CLI guide for agents
---

# CLI Guide

Prefer the CLI when it is installed; otherwise apply the docs manually.

```sh
crabenv --help        # inspect current commands/options
crabenv init          # create/align schema + .env.example
crabenv copy          # create/update local .env from .env.example
crabenv ls -p         # print expanded env inventory; use this for agents/scripts
crabenv doctor        # detect drift, common mistakes, and managed sink drift
crabenv doctor --fix  # preview safe fixes
crabenv doctor --fix --yes # apply safe fixes
```

GitHub Actions sinks are supported through managed `gha-env` and `gha-echo` blocks. See [Sinks](./sinks/index.md).
When a managed sink covers a schema variable, `crabenv list -p`/`crabenv ls -p` includes `sinks` in that variable's surfaces, and `crabenv doctor` marks the `sinks` checklist cell with `[x]`.

CRUD commands (wizard-like when used without args, but unusable for agents):

```sh
crabenv list -p
crabenv add VARIABLE_NAME --example "value" --optional
crabenv update VARIABLE_NAME
crabenv remove VARIABLE_NAME
```

Agent rule: run `crabenv --help` first, prefer `crabenv ls -p` over interactive `crabenv ls`, use the CLI for routine alignment, then edit files manually only when the CLI cannot express the needed change.

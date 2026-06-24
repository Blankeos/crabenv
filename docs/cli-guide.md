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
crabenv doctor        # detect drift, common mistakes, and managed sink drift
crabenv doctor --fix  # preview safe fixes
crabenv doctor --fix --yes # apply safe fixes
```

GitHub Actions sinks are supported through managed `gha-env` and `gha-echo` blocks. See [Sinks](./sinks/index.md).
When a managed sink covers a schema variable, `crabenv list`/`crabenv ls` includes `sinks` in that variable's surfaces, and `crabenv doctor` marks the `sinks` checklist cell with `[x]`.

CRUD commands (wizard-like when used without args, but unusable for agents):

```sh
crabenv list
crabenv add VARIABLE_NAME --example "value" --optional
crabenv update VARIABLE_NAME
crabenv remove VARIABLE_NAME
```

Agent rule: run `crabenv --help` first, use the CLI for routine alignment, then edit files manually only when the CLI cannot express the needed change.

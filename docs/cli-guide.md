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
crabenv doctor        # detect drift and common mistakes
crabenv doctor --fix  # apply safe fixes
```

CRUD commands (wizard-like when used without args, but unusable for agents):

```sh
crabenv list
crabenv add VARIABLE_NAME --example "value" --optional
crabenv update VARIABLE_NAME
crabenv remove VARIABLE_NAME
```

Agent rule: run `crabenv --help` first, use the CLI for routine alignment, then edit files manually only when the CLI cannot express the needed change.

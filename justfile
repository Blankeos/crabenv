default:
    just --list

dev:
    cargo r

dist-build *args:
    dist build {{ args }}

sync_readme:
    cp README.md npm/README.md

# Release: bump versions, create release commit, and create a git tag.

# Usage: just tag [patch|minor|major]
tag bump="":
    sh scripts/tag_and_release.sh {{ bump }}

devdocs:
    bunx gittydocs dev docs

generate-skill:
    bun scripts/generate-skill.ts

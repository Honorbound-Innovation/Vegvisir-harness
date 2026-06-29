#!/usr/bin/env bash
set -euo pipefail

# Find likely entrypoints and build files.
# Usage: ./ventrypoints.sh [path]

root="${1:-.}"

find "$root" \
  -path '*/.git' -prune -o \
  -path '*/.vegvisir' -prune -o \
  -type f \( \
    -name 'Cargo.toml' -o -name 'package.json' -o -name 'pyproject.toml' -o \
    -name 'Makefile' -o -name 'build.gradle' -o -name 'pom.xml' -o \
    -name 'main.*' -o -name 'index.*' -o -name 'app.*' \
  \) -print | sort

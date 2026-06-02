# Security Policy

Binary Intelligence Workbench is intended for authorized defensive analysis, reverse engineering, and research.

## Current MVP Safety Posture

- Static analysis only.
- Does not execute target binaries.
- Does not patch binaries.
- Does not collect credentials.
- Does not store plaintext secrets in memory.
- Ghidra integration uses read/extraction workflows.

## Handling Unknown Binaries

Treat unknown binaries as hostile. Do not execute them on the host. Use isolated sandboxes for any future dynamic-analysis feature.

## Reports and Memory

Reports may contain strings extracted from binaries. Review outputs before sharing or storing summaries in durable memory, especially for firmware/configuration images that may include secret-like material.

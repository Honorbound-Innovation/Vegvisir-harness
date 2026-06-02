# Skill: artifact.safe_extract_inventory

## Purpose
Safely extract and inventory archives or bundled evidence without executing contained files, while preserving provenance and producing reproducible case artifacts.

## Use When
- The artifact is a `.zip`, `.tar`, `.gz`, `.7z`, nested archive, challenge bundle, evidence package, or suspicious attachment.
- You need hashes, file tree, types, and extraction notes before deeper analysis.

## Inputs
- `archive_path`
- `case_output_dir`
- `known_archive_passwords` or permitted password strategy, when authorized
- `max_extract_size`
- `preserve_permissions`: boolean

## Procedure
1. Hash the original archive before extraction.
2. List archive contents before extracting when possible.
3. Extract into a scoped case/sample directory, never over the original source directory.
4. Use known challenge passwords only when appropriate and non-secret by convention; do not ask for plaintext secrets in chat.
5. Prevent path traversal and unsafe absolute paths.
6. Inventory extracted files:
   - relative path
   - size
   - SHA256
   - file type
   - executable bit
   - nested archive markers
   - suspicious extensions or magic bytes
7. Normalize execute permissions only on scoped copies and only when needed for authorized analysis.
8. Identify likely downstream skills.

## Output Contract
Create or return:
- `inventory.json`
- `hashes.json`
- `file_tree.txt`
- `suspicious_artifacts.json`
- `extraction_notes.md`
- recommended downstream skill list

## Safety Boundary
Never execute extracted files during this skill. Do not extract over user work. Stop on path traversal, decompression bomb indicators, or ambiguous password/secret handling.

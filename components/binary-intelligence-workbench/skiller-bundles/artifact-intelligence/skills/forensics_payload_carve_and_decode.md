# Skill: forensics.payload_carve_and_decode

## Purpose
Extract, carve, decode, and safely inspect embedded payloads from captures, streams, archives, binaries, scripts, or incident artifacts.

## Use When
- Evidence includes HTTP bodies, raw TCP streams, binary blobs, scripts, base64/hex data, compressed data, RDB transfers, or embedded shared objects.
- You need to recover staged payloads, scripts, modules, or encoded command output.

## Inputs
- `source_artifact_or_stream`
- `case_output_dir`
- optional `known_offsets_or_markers`
- optional `decode_hints`

## Procedure
1. Preserve original source artifact and hash it.
2. Identify carve markers by magic bytes, protocol metadata, content length, stream boundaries, or embedded headers.
3. Carve payloads into `carved_artifacts/` with hashes and provenance.
4. Classify carved artifacts by file type and entropy.
5. Decode safe encodings:
   - hex
   - base64
   - URL encoding
   - shell escapes
   - gzip/zip/tar when safe
6. Deobfuscate scripts by reconstructing strings without executing commands.
7. Identify encryption, keys, IVs, algorithms, or command-output formats if present.
8. Route carved binaries to `binary.static_triage` and network evidence to protocol-specific skills.

## Output Contract
- `carved_artifacts/`
- `decoded_payloads/`
- `payload_index.json`
- `decode_steps.md`
- `ioc_candidates.json`
- `safety_notes.md`

## Safety Boundary
Never execute decoded payloads during this skill. Avoid expanding archives with unsafe paths or excessive size. Treat carved payloads as malicious until proven otherwise.

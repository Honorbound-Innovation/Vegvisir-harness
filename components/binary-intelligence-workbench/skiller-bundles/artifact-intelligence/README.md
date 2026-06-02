# Artifact Intelligence Skiller Bundle

Generalized Vegvisir/BIW skills for analyzing authorized adversarial technical artifacts: archives, binaries, packet captures, payloads, crypto oracles, incident evidence, and reports.

This bundle was generalized from validated BIW workflows, but the included examples are sanitized and do not contain flags, credentials, tokens, private keys, or recovered secrets.

## Skills

- `artifact.triage_and_route`
- `artifact.safe_extract_inventory`
- `binary.static_triage`
- `binary.validation_logic_recovery`
- `binary.control_flow_deobfuscation`
- `binary.vm_lift_and_reduce`
- `forensics.network_capture_triage`
- `incident.redis_protocol_abuse`
- `forensics.payload_carve_and_decode`
- `crypto.fault_oracle_analysis`
- `crypto.aes_dfa_key_recovery`
- `report.professional_writeup_generate`

## Operating posture

- Authorized CTF/lab/owned/defensive targets only.
- Preserve original evidence.
- Write derived artifacts only to scoped case directories.
- Use static/read-only inspection first.
- Dynamic execution, debugger tracing, external endpoints, and crypto oracle interaction require explicit scope.
- Do not store recovered flags, credentials, keys, tokens, or private data in durable memory.

## Validation

The bundle currently validates and passes deterministic Skiller evals. Publication readiness remains blocked until human review/approval and maturity promotion, especially for `crypto.aes_dfa_key_recovery`.


## Multi-stage chain skills added from CCT-style generalized workflows

Added approved generalized skills for multi-stage evidence chains, image stego chains, classical cipher chains, .NET GUI constraint recovery, and PCAP embedded-artifact chains. These skills are generalized patterns for authorized artifact analysis and intentionally avoid storing task-specific flags or secrets.

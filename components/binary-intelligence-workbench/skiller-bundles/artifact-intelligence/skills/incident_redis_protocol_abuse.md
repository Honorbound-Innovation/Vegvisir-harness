# Skill: incident.redis_protocol_abuse

## Purpose
Analyze Redis network evidence and reconstruct abuse chains such as unauthorized access, rogue replication, configuration changes, malicious module loading, custom command execution, and cleanup.

## Use When
- A PCAP or log set contains Redis RESP traffic.
- Evidence includes `AUTH`, `CONFIG SET`, `SLAVEOF`/`REPLICAOF`, `MODULE LOAD`, RDB/module transfer, or unknown Redis commands.
- The user needs attack timeline, IOCs, remediation, or payload recovery.

## Inputs
- `pcap_or_streams_dir`
- `case_output_dir`
- optional `redis_port_hint`
- optional `carving_allowed`

## Procedure
1. Extract Redis streams and parse RESP commands/responses.
2. Build a timeline of client/server interactions.
3. Identify authentication and privilege context without storing plaintext credentials in memory.
4. Detect dangerous commands:
   - `CONFIG SET dir`
   - `CONFIG SET dbfilename`
   - `SLAVEOF` / `REPLICAOF`
   - `MODULE LOAD` / `MODULE UNLOAD`
   - custom commands registered by modules
5. Reconstruct rogue replication transfers and carve transferred artifacts when possible.
6. Route carved shared objects or binaries to `binary.static_triage`.
7. Identify command output encoding/encryption and route to payload/decode workflow if needed.
8. Produce attack chain, IOCs, and remediation guidance.

## Output Contract
- `redis_timeline.json`
- `redis_commands.json`
- `attack_chain.md`
- `carved_artifacts_index.json`
- `module_analysis_notes.md`
- `ioc_report.md`
- `remediation.md`

## Remediation Checklist
- Rotate Redis password/ACL secrets.
- Disable public exposure; bind to trusted interfaces.
- Enforce TLS where applicable.
- Disable or restrict module loading.
- Audit `dir`/`dbfilename` changes and replication settings.
- Review persistence files, authorized keys, cron, systemd units, MOTD/profile scripts, and staged payloads.
- Patch Redis and host OS.

## Safety Boundary
Do not reuse captured credentials. Do not contact attacker infrastructure. Decode and inspect payloads; do not execute them outside an explicit malware-analysis sandbox.

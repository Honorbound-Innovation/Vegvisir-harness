# Sanitized Example: Redis PCAP Intrusion

A packet capture contains Redis authentication followed by replication abuse, configuration changes, malicious module loading, custom command execution, staged payload downloads, and cleanup.

Generalized workflow:
1. Profile PCAP and identify Redis streams.
2. Parse RESP commands and build a timeline.
3. Detect `SLAVEOF`/`REPLICAOF`, `CONFIG SET`, `MODULE LOAD`, and custom commands.
4. Carve transferred module or RDB payloads.
5. Reverse carved module to understand command output encoding.
6. Decode payloads and command output safely.
7. Assemble evidence, IOCs, and remediation.

Credentials, flags, and environment-specific secrets must be redacted.

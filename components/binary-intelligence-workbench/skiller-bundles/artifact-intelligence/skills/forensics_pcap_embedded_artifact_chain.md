# forensics.pcap_embedded_artifact_chain

PCAP embedded artifact chain analysis

Use this skill for packet captures that contain embedded files, nested captures, covert-channel messages, encrypted transfers, protocol clues, hashes, or payloads that require follow-on analysis. It generalizes USBPcap extraction, network PCAP triage, ICMP/DNS/TCP covert data, cryptcat-like encrypted streams, and decrypted binary follow-up.

## Guardrails

- Do not replay traffic to external systems unless explicitly authorized.
- Prefer offline extraction and analysis.
- Do not execute decrypted payloads without sandbox approval.
- Preserve original captures and record exact extraction filters/fields.

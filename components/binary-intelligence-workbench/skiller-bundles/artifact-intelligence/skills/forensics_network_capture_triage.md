# Skill: forensics.network_capture_triage

## Purpose
Perform first-pass forensic triage of packet captures, identify protocols/conversations/streams of interest, and route toward incident reconstruction, payload carving, or protocol-specific analysis.

## Use When
- The artifact is a `.pcap` or `.pcapng` file.
- The user provides an incident narrative or asks for forensic reconstruction.
- You need to identify suspicious conversations and extract evidence without executing payloads.

## Inputs
- `pcap_path`
- `case_output_dir`
- optional `challenge_or_incident_context`
- optional `tshark_available`

## Procedure
1. Compute PCAP hash and basic metadata.
2. Generate protocol hierarchy, endpoint, conversation, and stream summaries.
3. Identify notable services: HTTP, DNS, SMB, FTP, SSH, Redis, database protocols, TLS, unknown TCP/UDP.
4. Extract HTTP object metadata when available.
5. Index TCP streams and candidate payload-transfer streams.
6. Look for auth events, suspicious commands, file transfer, staging, exfiltration, and cleanup.
7. Route protocol-specific traffic to specialized skills.

## Output Contract
- `pcap_profile.json`
- `protocols.json`
- `conversations.json`
- `streams_index.json`
- `http_objects_index.json`
- `suspicious_activity.md`
- `recommended_next_skills.json`

## Safety Boundary
Offline packet inspection only. Do not replay traffic or contact observed infrastructure unless explicitly authorized.

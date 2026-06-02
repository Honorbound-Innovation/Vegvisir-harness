# Skill: artifact.triage_and_route

## Purpose
Safely triage an unknown technical artifact, classify its likely domain, inventory available evidence, identify risks, and route to the most appropriate specialized analysis workflow.

This is the front-door workflow for mixed CTF, incident-response, reverse-engineering, malware-analysis, firmware, crypto, and forensic packages.

## Use When
- The input is an unknown archive, binary, packet capture, source bundle, memory/log artifact, or service endpoint.
- The user provides challenge or incident context but the correct analysis path is not yet obvious.
- Multiple tools or downstream skills may apply.

## Inputs
- `artifact_path`: path to local artifact, directory, archive, binary, pcap, source tree, or evidence bundle.
- `optional_context`: challenge statement, incident narrative, user notes, suspected platform, or known constraints.
- `optional_endpoint`: host/port or service URL when explicitly authorized.
- `allowed_tools`: available local tools and any approved network/debugger/sandbox capabilities.
- `case_output_dir`: directory where evidence and derived artifacts should be written.
- `risk_mode`: static-only, sandboxed-dynamic, live-service, or report-only.

## Procedure
1. Establish authorization and scope from user context. If scope is unclear, stop before external or destructive actions.
2. Create or select a scoped case directory. Avoid modifying source artifacts.
3. Inventory the artifact without execution:
   - filename, size, hashes, MIME/file type
   - archive contents, nested artifacts, permissions
   - binary/source/pcap/log indicators
   - timestamps and metadata when available
4. Classify likely domain(s):
   - `archive`, `binary`, `pcap`, `source`, `firmware`, `crypto-service`, `incident-evidence`, `mixed`
5. Identify high-value evidence and immediate safety constraints.
6. Route to one or more specialized skills:
   - archive or nested files: `artifact.safe_extract_inventory`
   - executable binary: `binary.static_triage`
   - password/license/flag validation: `binary.validation_logic_recovery`
   - misleading/decompiler-hostile control flow: `binary.control_flow_deobfuscation`
   - suspected custom VM: `binary.vm_lift_and_reduce`
   - packet capture: `forensics.network_capture_triage`
   - Redis traffic: `incident.redis_protocol_abuse`
   - embedded payloads/scripts/modules: `forensics.payload_carve_and_decode`
   - correct/faulty crypto output: `crypto.fault_oracle_analysis`
   - AES fault setting: `crypto.aes_dfa_key_recovery`
   - finished case: `report.professional_writeup_generate`
7. Produce a short, evidence-backed analysis plan with exact next commands or tool actions.

## Output Contract
Return Markdown and/or JSON containing:
- `case_id`
- `artifact_inventory_summary`
- `classification`
- `risk_notes`
- `recommended_skills`
- `analysis_plan`
- `evidence_needed`
- `blocked_items`
- `exact_next_steps`

## Quality Bar
- Do not over-classify from weak evidence.
- Prefer reversible, static-first inspection.
- Preserve original artifacts.
- Make routing explainable from observed evidence.

## Safety Boundary
Do not execute unknown binaries, contact external endpoints, load kernel modules, import malware, or decrypt/handle secrets outside explicit user authorization and appropriate isolation. Never store flags, credentials, tokens, or secret-like values in durable memory.

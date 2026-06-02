# Skill: binary.control_flow_deobfuscation

## Purpose
Analyze binaries where normal disassembly or decompilation is misleading due to control-flow obfuscation, call-threading, return-threading, dispatcher loops, opaque predicates, fake function boundaries, or VM-style execution.

## Use When
- Ghidra/decompiler output is incomplete, nonsensical, or misses the real entry logic.
- The binary uses patterns such as `call target; pop reg`, indirect dispatch, computed jumps, tiny blocks, or abnormal function boundaries.
- Static function recovery fails but runtime behavior is observable in a controlled environment.

## Inputs
- `case_dir`
- `binary_path`
- `entrypoint_or_main_hint`
- `strings_and_xrefs`
- optional `debugger_available`
- optional `sandbox_profile`

## Procedure
1. Identify the true execution path from entrypoint, `__libc_start_main`, or platform-specific startup.
2. Compare static/decompiler function boundaries with runtime control flow.
3. Detect control-flow threading patterns:
   - call-threading
   - return-threading
   - dispatcher loops
   - fake call/return state manipulation
4. Avoid trusting decompiler pseudocode when function recovery is demonstrably wrong.
5. Capture a bounded runtime trace after input or key event, if authorized.
6. Reduce trace noise:
   - remove call-threading scaffolding
   - collapse repeated MBA helper blocks when possible
   - isolate semantic loads/stores/arithmetic/branches
7. Determine whether a custom VM/state machine exists.
8. If VM-like, route to `binary.vm_lift_and_reduce`.
9. Produce a control-flow model and recommended semantic recovery path.

## Output Contract
- `control_flow_model.md`
- `trace_summary.json`
- `obfuscation_patterns.json`
- `semantic_blocks.md`
- `vm_candidate.json`
- `recommended_next_steps.md`

## Quality Bar
- Explicitly call out decompiler failure modes.
- Favor executed evidence over linear disassembly.
- Keep traces bounded and reproducible.

## Safety Boundary
Dynamic tracing requires explicit authorization and an appropriate sandbox. Do not run suspicious binaries on sensitive hosts.

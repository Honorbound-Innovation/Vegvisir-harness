# Skill: binary.vm_lift_and_reduce

## Purpose
Lift a custom virtual machine, bytecode interpreter, or stack/register-machine validator from traces or disassembly into a simplified intermediate representation suitable for reasoning, solving, or reporting.

## Use When
- A binary contains a dispatcher loop, opcode table, repeated handler pattern, or VM-like state.
- Validation/output logic is hidden behind bytecode or obfuscated helper operations.
- Runtime traces reveal repeated instruction motifs over an input-dependent stack or state array.

## Inputs
- `case_dir`
- `trace_file`
- `candidate_vm_state_locations`
- `opcode_table_or_dispatch_hints`
- optional `known_input_buffer`
- optional `target_predicate_location`

## Procedure
1. Identify VM state:
   - instruction pointer / bytecode pointer
   - stack pointer
   - value stack or register file
   - input buffer references
   - constants/opcode table
2. Segment trace by dispatch iteration or handler boundary.
3. Infer opcode semantics by observing stack/register effects.
4. Simplify inflated operations such as MBA expressions into canonical operations (`xor`, `or`, `add`, `sub`, `mul`, comparisons).
5. Emit a lifted IR or pseudocode model.
6. Identify target predicate or output construction.
7. Solve simple constraints directly; route complex constraints to a solver when available.
8. Verify recovered input/output against the original binary in a sandbox when authorized.

## Output Contract
- `vm_program.json`
- `opcode_semantics.json`
- `lifted_ir.py` or `lifted_ir.md`
- `constraints.md` or `constraints.py`
- `solve_notes.md`
- `verification_results.md`

## Quality Bar
- Every opcode semantic must be backed by trace evidence or black-box tests.
- Preserve uncertainty for partially inferred handlers.
- Validate the lifted model against at least one observed execution when practical.

## Safety Boundary
This skill may involve dynamic tracing. Use only in authorized, controlled environments.

# Skill: crypto.fault_oracle_analysis

## Purpose
Analyze cryptographic systems that expose correct/faulty outputs, error-dependent behavior, or repeatable oracle responses, and determine whether a practical cryptanalytic attack applies.

## Use When
- Source, binary, or service exposes both correct and faulted ciphertexts.
- The system leaks padding, timing, error messages, or differential behavior.
- The challenge or incident involves custom crypto, deterministic misuse, or fault injection.

## Inputs
- `source_or_binary_or_endpoint`
- `oracle_description`
- `sample_transcripts`
- optional `allowed_remote_interaction`
- optional `max_queries`

## Procedure
1. Identify primitive, mode, key reuse, IV/nonce behavior, and output format.
2. Determine oracle type:
   - correct/faulty ciphertext pair
   - padding/error oracle
   - timing oracle
   - bit-flip response
   - deterministic encryption misuse
3. Model what information the oracle leaks.
4. Decide whether a known attack family applies.
5. Collect bounded samples if remote interaction is authorized.
6. Derive equations or constraints from the leak.
7. Implement or adapt a solver with test vectors when practical.
8. Verify recovered material by decrypting, authenticating, or replaying only within scope.

## Output Contract
- `oracle_model.md`
- `sample_pairs.json`
- `attack_strategy.md`
- `solver.py` or `solver_notes.md`
- `recovered_material.json` when allowed
- `verification.md`

## Safety Boundary
Authorized services/labs only. Do not attack third-party cryptographic systems without explicit permission. Do not store recovered secrets in memory.

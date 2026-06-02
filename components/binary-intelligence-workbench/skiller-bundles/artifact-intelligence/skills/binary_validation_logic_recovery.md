# Skill: binary.validation_logic_recovery

## Purpose
Recover password, serial, license, unlock, or flag-validation logic from a binary using evidence-backed static and controlled dynamic analysis.

## Use When
- A binary prompts for user input and reports success/failure.
- Strings or symbols suggest `strcmp`, `memcmp`, `strncmp`, hash checks, encoded constants, or validation arrays.
- You need candidate inputs or reconstructed output material.

## Inputs
- `case_dir`
- `binary_path`
- `static_artifacts`
- optional `allowed_dynamic_execution`
- optional `sandbox_profile`

## Procedure
1. Review prompt, success, and failure strings.
2. Locate cross-references to validation-related APIs and strings.
3. Inspect symbols and decompile likely validation functions when available.
4. Recover direct constants, arrays, tables, and transforms.
5. Determine whether input is compared directly, hashed, transformed, or used to derive output.
6. If authorized, run controlled verification with candidate inputs.
7. Record both the recovered logic and the evidence path.
8. Prefer minimal verification commands over broad execution.

## Output Contract
- `validation_logic.md`
- `candidate_inputs.json`
- `recovered_constants.json`
- `verification_commands.md`
- `verification_results.md`

## Quality Bar
- Distinguish password/input from final output/flag/result.
- Verify candidates when safe.
- Explain success/failure path clearly.

## Safety Boundary
Do not bypass access controls on unauthorized software. Use for CTFs, owned binaries, lab targets, malware config recovery, or defensive analysis only.

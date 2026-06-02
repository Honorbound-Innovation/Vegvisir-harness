# Skill: crypto.aes_dfa_key_recovery

## Purpose
Recover AES keys from correct/faulty ciphertext pairs when the fault model is compatible with Differential Fault Analysis, especially late-round single-byte faults.

## Use When
- You have AES correct and faulty ciphertext pairs under the same key.
- Faults occur before a late-round MixColumns or near the final rounds.
- Source or behavior suggests a single-byte random fault in AES state.

## Inputs
- `sample_pairs`: correct/faulty ciphertext pairs
- `encrypted_target`: ciphertext to decrypt after key recovery
- `aes_variant`: AES-128/192/256 when known
- `mode`: ECB/CBC/CTR/etc. when known
- optional `source_code`
- optional `remote_endpoint`
- optional `max_samples`

## Procedure
1. Confirm same-key oracle behavior and target block alignment.
2. Identify AES implementation state layout if source is available.
3. Determine fault round and affected byte pattern.
4. Group faulty ciphertexts by affected four-byte final-round column after ShiftRows.
5. For each affected group, recover last-round-key byte candidates by applying inverse S-box differential constraints.
6. Intersect candidates across enough samples to recover the full last-round key.
7. Invert the AES key schedule to recover the original key.
8. Decrypt the target ciphertext with the recovered key and correct mode.
9. Validate with local test vectors or service output.

## Output Contract
- `fault_pairs.json`
- `fault_grouping.md`
- `round_key_candidates.json`
- `recovered_round_key.json`
- `recovered_key.json`
- `decryptor.py`
- `attack_report.md`
- `verification.md`

## Common Pitfalls
- Confusing AES row-major vs column-major state layout.
- Applying ShiftRows mapping in the wrong direction.
- Treating observed ciphertext byte order as internal state order.
- Using too few samples for candidate intersection.
- Forgetting to invert the key schedule from the recovered last-round key.
- Assuming PKCS#7 padding when the program uses zero padding.

## Safety Boundary
Use only on authorized challenge services, lab systems, or owned cryptographic implementations. Do not store recovered keys or secret outputs in durable memory.

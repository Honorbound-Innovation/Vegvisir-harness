# Sanitized Example: AES Fault Oracle

A service returns both correct and faulty AES ciphertexts under a stable unknown key, then provides an encrypted target value.

Generalized workflow:
1. Confirm primitive, key reuse, and fault timing.
2. Collect correct/faulty ciphertext pairs.
3. Group affected ciphertext bytes by final-round column.
4. Use inverse S-box differential constraints to recover last-round-key candidates.
5. Intersect candidates across samples.
6. Invert AES key schedule.
7. Decrypt the target ciphertext.
8. Verify output format.

Recovered keys and challenge outputs should not be stored in durable memory.

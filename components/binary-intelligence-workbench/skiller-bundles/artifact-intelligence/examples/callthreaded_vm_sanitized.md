# Sanitized Example: Call-Threaded VM Binary

A stripped ELF resists normal decompilation because the true execution path is composed of many tiny blocks connected by call-threading. Static function boundaries are misleading.

Generalized workflow:
1. Find true entry/main from runtime startup arguments.
2. Identify call-threading pattern such as `call target; pop reg`.
3. Capture bounded dynamic trace after input.
4. Remove threading scaffolding.
5. Identify VM state arrays, opcode table, stack pointer, and input buffer reads.
6. Infer opcode semantics from stack effects.
7. Reduce final predicate to simple constraints.
8. Verify recovered input/output against the binary.

No challenge flags or secret-like outputs are stored in this example.

# binary.dotnet_gui_constraint_recovery

.NET GUI constraint and key recovery

Use this skill for .NET/Mono GUI binaries where a key, license, or flag is produced after UI controls satisfy constraints. It supports static recovery of slider/textbox/button conditions, embedded byte arrays, XOR/transform logic, and success/failure message generation without requiring Windows GUI interaction.

## Guardrails

- Do not run unknown GUI binaries outside authorized/sandboxed context.
- Do not patch binaries unless explicitly requested and backed up.
- Keep recovered secret-like values out of durable memory/examples.

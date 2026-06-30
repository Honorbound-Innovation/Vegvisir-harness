# Privileged command workflow

Vegvisir supports privileged local commands without allowing the user's sudo password to enter chat, model context, session history, run artifacts, or trace logs.

## Security invariant

Vegvisir must never send, store, echo, serialize, trace, or forward the sudo password through chat, provider/model context, model-callable tool arguments, normal command history, run artifacts, or telemetry.

`/sudo auth` is a local TUI-only workflow. The default path opens an in-Vegvisir secure password modal whose buffer is separate from the normal chat input. The modal masks the password, writes it only to `sudo` stdin for `sudo -S -p '' -v`, then clears the transient buffer. Privileged tool execution then uses `sudo -n`, which consumes only an existing sudo timestamp and fails closed if authentication is not already cached.

A terminal fallback remains available with `/sudo auth --terminal`; that path temporarily leaves raw/alternate-screen TUI mode and delegates password entry directly to the OS `sudo` prompt.

## User flow

1. In the local TUI, run:

   ```text
   /sudo auth
   ```

2. Vegvisir opens a secure modal prompt inside the TUI.

   - Typed characters go into `SudoPasswordPrompt`, not the normal chat input.
   - The screen renders bullets only.
   - The prompt is not added to command history or session messages.
   - `/sudo` command telemetry redacts arguments.
   - Paste events while the modal is open are routed into the secure prompt buffer, not chat.
   - `Esc` / `Ctrl-C` cancels and clears the buffer.

3. Press Enter. Vegvisir runs:

   ```text
   sudo -S -p '' -v
   ```

   The password is written only to that child process stdin. stdout/stderr are null. The temporary encoded bytes and char buffer are cleared after the attempt.

4. After successful authentication, model/tool calls that need privilege use:

   ```text
   run_privileged_command { "command": ["<program>", "<args>"], ... }
   ```

   Vegvisir internally runs the command as:

   ```text
   sudo -n <program> <args>
   ```

   If the sudo timestamp expired or does not exist, the command fails with `SudoAuthenticationRequired` and instructs the user to run `/sudo auth`.

5. To invalidate the cached sudo timestamp, run:

   ```text
   /sudo clear
   ```

## Fallback flow

If the in-app prompt is unsuitable for a specific terminal, run:

```text
/sudo auth --terminal
```

That path temporarily leaves the TUI and runs `sudo -v` attached to the controlling terminal. `sudo` owns the prompt. Vegvisir does not read or capture the password and only observes the exit status.

## Guardrails

- `run_command ["sudo", ...]` is rejected. Use `/sudo auth` and `run_privileged_command` instead.
- Normal command/test paths reject nested sudo invocations such as shell snippets containing `sudo`.
- `run_privileged_command` rejects commands that already include `sudo`; the tool adds `sudo -n` internally.
- Model-callable command tools remain non-interactive and set child stdin to null.
- `/sudo` command telemetry redacts arguments to protect accidental input such as `/sudo auth <password>`.
- Existing risky-tool approval, allow-list, network-approval, runtime-policy, and sandbox gates still apply.

## Operational notes

- `/sudo status` reports whether a sudo timestamp is currently valid.
- `/sudo auth` requires the local interactive TUI.
- This workflow is local-host only. Remote bridge or MCP integrations should use HBSE/service-account mechanisms rather than plaintext sudo passwords.
- If a privileged command requires interactive input other than sudo authentication, it should be run manually or through a purpose-built interactive workflow; generic tools are intentionally non-interactive.

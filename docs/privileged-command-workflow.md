# Privileged command workflow

Vegvisir supports privileged local commands without allowing the user's sudo password to enter chat, model context, session history, run artifacts, or trace logs.

## Security invariant

Vegvisir must never ask for, read, store, echo, serialize, or forward the sudo password.

Password entry is delegated directly to the operating system's `sudo` prompt on the controlling terminal. Privileged tool execution then uses `sudo -n`, which consumes only an existing sudo timestamp and fails closed if authentication is not already cached.

## User flow

1. In the TUI, run:

   ```text
   /sudo auth
   ```

2. Vegvisir temporarily leaves raw/alternate-screen TUI mode and runs:

   ```text
   sudo -v
   ```

   `sudo` owns the terminal prompt. Vegvisir does not pipe stdin, does not capture the password, and only observes the exit status.

3. After successful authentication, model/tool calls that need privilege use:

   ```text
   run_privileged_command { "command": ["<program>", "<args>"], ... }
   ```

   Vegvisir internally runs the command as:

   ```text
   sudo -n <program> <args>
   ```

   If the sudo timestamp expired or does not exist, the command fails with `SudoAuthenticationRequired` and instructs the user to run `/sudo auth`.

4. To invalidate the cached sudo timestamp, run:

   ```text
   /sudo clear
   ```

## Guardrails

- `run_command ["sudo", ...]` is rejected. Use `/sudo auth` and `run_privileged_command` instead.
- `sudo -S` / `sudo --stdin` patterns are rejected to prevent passwords being piped through tool args, shell scripts, chat, or traces.
- `run_privileged_command` rejects commands that already include `sudo`; the tool adds `sudo -n` internally.
- Bounded command execution sets child stdin to null for normal, test, task, and privileged command runners.
- Existing risky-tool approval, allow-list, network-approval, runtime-policy, and sandbox gates still apply.

## Operational notes

- `/sudo status` reports whether a sudo timestamp is currently valid.
- `/sudo auth` requires an interactive terminal.
- This workflow is local-host only. Remote bridge or MCP integrations should use HBSE/service-account mechanisms rather than plaintext sudo passwords.
- If a privileged command requires interactive input other than sudo authentication, it should be run manually or through a purpose-built interactive workflow; generic tools are intentionally non-interactive.

# USRL Usage

USRL provides contract-style governance for tightly bounded workflows. Vegvisir uses USRL contracts for specialized skills, risky tool gates, staged execution, required evidence, and regulated agent behavior.

The implementation is stored in:

```text
components/usrl
```

## Build

```bash
cd components/usrl
npm install
npm run build
```

## Test

```bash
npm test
```

## CLI

After building:

```bash
node dist/src/cli.js --help
```

## Vegvisir Integration

USRL contracts can be bound to agents:

```text
/agent bind-usrl agent-red security-audit
/agent unbind-usrl agent-red security-audit
```

USRL-backed skills should define the workflow constraints the runtime must enforce, including:

- allowed operation type
- allowed tool set
- stage requirements
- preconditions
- required evidence
- denied targets
- approval requirements

For high-risk tasks, USRL should narrow what is permitted even when an agent has broad capability.


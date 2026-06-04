# Memory Model

Vegvisir uses CMS-v2 for durable memory and ECM for active context exposure. Project memory is the default; global/user memory is explicit. ChatGPT archive retrieval is explicit-only.

Operator commands:

- `/context explain <message>`
- `/context last`
- `/context diff-last`
- `/context budget <message>`
- `/context sources <message>`
- `/memory used-this-turn`
- `/memory writes-this-session`
- `/memory why <memory-id>`
- `/memory diff <a> <b>`
- `/memory export [--global] [--out file]`
- `/memory quarantine <id>`
- `/memory forget <id>`

Plaintext secrets are rejected for durable memory writes and redacted from artifacts.

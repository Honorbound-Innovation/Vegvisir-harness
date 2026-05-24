23ChatGPT

I’ll add the missing library mechanics: indexed sub-skill functions with routing metadata, dependency graphs, eval hooks, and token-budgeted loading.

Correct — use .lsl = Linked Skill Library.

That is a better abstraction than treating skills as isolated prompt files. For Vegvisir-harness, an .lsl file should represent a primary skill namespace that contains many linked sub-skills, where each sub-skill behaves like a callable function with metadata, dependencies, routing rules, eval hooks, and token-budgeted loading.

The model should almost never load the full .lsl. It should load:

library header → sub-skill index → selected sub-skill signatures → required bodies → linked dependencies

Not:

entire cryptography skill blob

Vegvisir .lsl Concept

A .lsl file should be a structured procedural library.

Example:

skills/
  cryptography.lsl
  software_engineering.lsl
  document_processing.lsl
  reverse_engineering.lsl
  agent_orchestration.lsl

Inside cryptography.lsl, you might have:

cryptography
  ├── aes_256
  ├── rsa
  ├── elliptic_curves
  ├── bitcoin_wallets
  ├── key_derivation
  ├── hashing
  ├── secure_randomness
  ├── threat_modeling
  └── implementation_pitfalls

Each of those is a sub-skill, not a separate monolithic skill.

The runtime should treat those sub-skills as callable units:

cryptography.aes_256
cryptography.bitcoin_wallets
cryptography.key_derivation.pbkdf2
cryptography.key_derivation.argon2id
cryptography.hashing.sha256

This gives Vegvisir the structure of a skill library rather than a flat prompt folder.
Core Design Shift

Instead of:

Skill = one prompt document

Use:

Linked Skill Library = namespace
Sub-skill = callable procedural unit
Skill link = dependency/reference between sub-skills
Skill router = loader that selects only the needed subgraph

So a primary skill is not the instruction itself. It is a container and namespace.

A sub-skill is the real operational unit.
Recommended .lsl Anatomy

A Linked Skill Library should contain these major sections:

@library
@index
@policy
@subskill
@link
@eval
@assets
@changelog

Conceptually:

cryptography.lsl
  @library       metadata for the full library
  @index         compact searchable map of sub-skills
  @policy        safety, risk, approval, and allowed usage constraints
  @subskill      individual callable skill functions
  @link          dependency graph between sub-skills
  @eval          tests/evaluation cases
  @assets        optional templates, snippets, references
  @changelog     version lineage

.lsl File Format

I would make .lsl a markdown-compatible, parseable format.

Not pure YAML.
Not pure JSON.
Not freeform markdown.

A good .lsl format should be:

frontmatter + declarative blocks + markdown bodies

Example:

---
lsl_version: 0.1
library: cryptography
display_name: Cryptography
version: 1.0.0
status: active
risk_level: high
owner: vegvisir
created_at: 2026-05-24T00:00:00Z
updated_at: 2026-05-24T00:00:00Z
---

@library
summary: >
  Procedures, cautions, implementation guidance, and diagnostic workflows
  for cryptographic systems.

load_policy:
  default_context_mode: index_only
  max_subskills_loaded: 4
  require_dependency_closure: true
  allow_extended_sections: false

@index
- id: cryptography.aes_256
  title: AES-256
  intent_tags: [aes, symmetric-encryption, block-cipher]
  summary: Use for AES-256 mode selection, implementation review, and safe usage patterns.
  risk: medium
  tokens:
    card: 80
    body: 700
    extended: 1800

- id: cryptography.bitcoin_wallets
  title: Bitcoin Wallets
  intent_tags: [bitcoin, wallets, addresses, bip32, bip39, bip44]
  summary: Use for explaining wallet structure, address derivation, and safe wallet design.
  risk: high
  tokens:
    card: 90
    body: 900
    extended: 2200

- id: cryptography.key_derivation.argon2id
  title: Argon2id Key Derivation
  intent_tags: [argon2id, password-hashing, key-derivation]
  summary: Use for password hashing and key derivation parameter guidance.
  risk: medium
  tokens:
    card: 75
    body: 650
    extended: 1300

Then define sub-skills:

@subskill cryptography.aes_256
title: AES-256
version: 1.0.0
risk: medium
type: procedure
activation:
  positive:
    - AES
    - AES-256
    - symmetric encryption
    - encrypt file
    - decrypt file
  negative:
    - crack AES
    - recover key
    - bypass encryption
requires:
  concepts:
    - cryptography.secure_randomness
    - cryptography.key_management
  tools: []
context_budget:
  card_tokens: 80
  body_tokens: 700
  extended_tokens: 1800

signature:
  name: cryptography.aes_256
  inputs:
    - name: task
      type: string
      required: true
    - name: mode
      type: enum
      values: [gcm, ctr, cbc, unknown]
      required: false
    - name: implementation_language
      type: string
      required: false
  outputs:
    - name: guidance
      type: procedural_steps
    - name: pitfalls
      type: list
    - name: verification
      type: checklist

body:
  Use AES-256 only with a safe mode and authenticated encryption where possible.

  Preferred default:
  - AES-256-GCM for authenticated encryption.
  - Unique nonce per encryption operation.
  - Strong random keys from a cryptographic RNG.
  - Clear separation between encryption keys and authentication material.

  Avoid:
  - ECB mode.
  - Reusing nonces in GCM.
  - Deriving encryption keys directly from passwords without a KDF.
  - Treating encryption as authentication.

verification:
  - Confirm the mode provides authentication or pair encryption with authentication.
  - Confirm nonce/IV uniqueness.
  - Confirm key generation source.
  - Confirm no hardcoded secret material.

Another example:

@subskill cryptography.bitcoin_wallets
title: Bitcoin Wallets
version: 1.0.0
risk: high
type: concept_procedure
activation:
  positive:
    - bitcoin wallet
    - BIP39
    - BIP32
    - seed phrase
    - address derivation
    - xpub
    - wallet architecture
  negative:
    - steal wallet
    - recover someone else's private key
    - bypass wallet security
    - drain funds
requires:
  concepts:
    - cryptography.secure_randomness
    - cryptography.key_derivation
    - cryptography.elliptic_curves.secp256k1
  tools: []
context_budget:
  card_tokens: 90
  body_tokens: 900
  extended_tokens: 2200

signature:
  name: cryptography.bitcoin_wallets
  inputs:
    - name: task
      type: string
      required: true
    - name: wallet_type
      type: enum
      values: [single_key, hd_wallet, multisig, unknown]
      required: false
    - name: network
      type: enum
      values: [mainnet, testnet, signet, regtest, unknown]
      required: false
  outputs:
    - name: explanation
      type: structured_explanation
    - name: safe_design_guidance
      type: checklist
    - name: risk_warnings
      type: list

body:
  Use this sub-skill for legitimate wallet architecture, education, safe implementation review,
  and controlled testnet/regtest workflows.

  Core concepts:
  - Wallets manage private keys or seed material.
  - HD wallets derive child keys from a master seed.
  - Public addresses are derived from public keys, not the reverse.
  - Private keys and seed phrases must never be exposed in logs, prompts, screenshots, or telemetry.

  Safe workflow:
  1. Determine whether the user is asking about concepts, implementation, testing, or recovery.
  2. Prefer testnet/signet/regtest examples for implementation tasks.
  3. Avoid handling real seed phrases or private keys.
  4. Explain key derivation and address generation without requesting secrets.
  5. Include storage, backup, and threat-modeling guidance.

forbidden:
  - Do not request or process real private keys.
  - Do not provide instructions for stealing, bypassing, or draining wallets.
  - Do not help recover a wallet that the user does not clearly own.
  - Do not embed real seed material in examples.

verification:
  - Confirm examples use dummy or testnet material.
  - Confirm no private key is requested.
  - Confirm wallet-security risks are explained.

Sub-Skills as Functions

A sub-skill should be function-like.

Each sub-skill needs:

id
title
version
risk level
activation rules
negative triggers
dependency links
input schema
output schema
body
verification checklist
failure modes
eval cases

A sub-skill is not just text. It is a callable procedural unit.

Equivalent Python-style interface:

class SubSkill:
    id: str
    title: str
    version: str
    risk_level: str

    activation: ActivationRules
    dependencies: list[str]
    required_tools: list[str]

    input_schema: dict
    output_schema: dict

    card: str
    body: str
    extended: str | None

    verification: list[str]
    forbidden: list[str]
    eval_refs: list[str]

Example callable identity:

cryptography.aes_256(
    task="review implementation",
    mode="gcm",
    implementation_language="python"
)

Or:

cryptography.bitcoin_wallets(
    task="explain HD wallet derivation",
    wallet_type="hd_wallet",
    network="testnet"
)

The model does not literally execute the sub-skill as code. But the harness should route, load, and apply it as though it were a callable function.
Linked Skill Graph

The “Linked” part of .lsl matters.

Sub-skills should reference other sub-skills explicitly.

Example:

@link
from: cryptography.bitcoin_wallets
to:
  - cryptography.key_derivation
  - cryptography.elliptic_curves.secp256k1
  - cryptography.secure_randomness
relationship: requires

@link
from: cryptography.aes_256
to:
  - cryptography.secure_randomness
  - cryptography.key_management
relationship: requires

@link
from: cryptography.key_derivation.argon2id
to:
  - cryptography.password_security
relationship: related

At runtime, Vegvisir should build a dependency closure.

Example request:

“Help me design a secure Bitcoin HD wallet flow.”

Router selection:

Primary:
  cryptography.bitcoin_wallets

Required dependencies:
  cryptography.secure_randomness
  cryptography.key_derivation
  cryptography.elliptic_curves.secp256k1

Optional related:
  cryptography.threat_modeling

Loaded context:

cryptography library card
cryptography.bitcoin_wallets body
cryptography.secure_randomness card/body
cryptography.key_derivation card/body
cryptography.elliptic_curves.secp256k1 card only

Not loaded:

AES-256
RSA
TLS
Hashing overview
Post-quantum crypto

That is the token economy win.
Loading Levels

Every .lsl library should support progressive loading.

Level 0: Library Card
Level 1: Sub-skill Index
Level 2: Sub-skill Cards
Level 3: Sub-skill Bodies
Level 4: Extended Notes / Examples / Eval References

Level 0 — Library Card

Small metadata only.

Library: cryptography
Purpose: cryptographic design, review, implementation guidance, and safety checks.
Sub-skills: aes_256, bitcoin_wallets, hashing, key_derivation, secure_randomness...
Risk: high

Level 1 — Sub-Skill Index

Compact searchable list.

cryptography.aes_256 — AES-256 usage, modes, pitfalls.
cryptography.bitcoin_wallets — HD wallets, seed phrases, address derivation.
cryptography.key_derivation — KDFs, password hashing, PBKDF2, Argon2id.

Level 2 — Sub-Skill Card

A compact callable description.

cryptography.aes_256
Use when discussing AES-256 encryption design or implementation.
Requires secure randomness and key management.
Avoid ECB, nonce reuse, and hardcoded keys.

Level 3 — Sub-Skill Body

The actual procedure.
Level 4 — Extended Notes

Examples, templates, eval cases, longer caveats, implementation checklists.

This lets Vegvisir load exactly what is needed.
.lsl Loader Runtime

The loader should work like this:

1. Parse all .lsl headers.
2. Build global library index.
3. Build sub-skill index.
4. Embed/search only cards and summaries.
5. Select candidate sub-skills.
6. Resolve required dependencies.
7. Enforce risk and policy gates.
8. Load selected bodies within token budget.
9. Apply sub-skills during planning/execution.
10. Record which sub-skills were used in trace memory.

Pseudocode:

def load_skills_for_task(task: str, token_budget: int) -> LoadedSkillContext:
    candidate_subskills = router.search_subskills(task)

    ranked = router.rank(
        task=task,
        candidates=candidate_subskills,
        signals=[
            "semantic_similarity",
            "intent_tags",
            "tool_compatibility",
            "success_score",
            "recent_success",
            "risk_level",
        ],
    )

    selected = budgeter.select_within_budget(
        ranked=ranked,
        token_budget=token_budget,
        max_primary_subskills=3,
    )

    dependency_closure = graph.resolve_required_dependencies(selected)

    gated = policy.enforce(
        task=task,
        subskills=selected + dependency_closure,
    )

    return loader.materialize(
        subskills=gated,
        loading_strategy="cards_then_bodies",
        token_budget=token_budget,
    )

.lsl Parser Model

Use a concrete internal model.

from dataclasses import dataclass, field
from typing import Literal, Any


@dataclass
class LinkedSkillLibrary:
    id: str
    name: str
    version: str
    status: Literal["active", "stale", "archived", "pinned"]
    risk_level: Literal["low", "medium", "high", "critical"]
    summary: str
    load_policy: dict[str, Any]
    subskills: dict[str, "SubSkill"]
    links: list["SkillLink"]
    evals: list["SkillEval"]
    changelog: list[dict[str, Any]]


@dataclass
class SubSkill:
    id: str
    title: str
    version: str
    type: Literal[
        "concept",
        "procedure",
        "diagnostic",
        "implementation",
        "review",
        "workflow",
        "policy",
    ]
    risk_level: Literal["low", "medium", "high", "critical"]
    activation: "ActivationRules"
    required_tools: list[str]
    dependencies: list[str]
    input_schema: dict[str, Any]
    output_schema: dict[str, Any]
    context_budget: "ContextBudget"
    card: str
    body: str
    extended: str | None
    forbidden: list[str]
    verification: list[str]
    failure_modes: list[str]
    metrics: dict[str, Any] = field(default_factory=dict)


@dataclass
class ActivationRules:
    positive: list[str]
    negative: list[str]
    required_context: list[str] = field(default_factory=list)


@dataclass
class ContextBudget:
    card_tokens: int
    body_tokens: int
    extended_tokens: int


@dataclass
class SkillLink:
    source: str
    target: str
    relationship: Literal[
        "requires",
        "related",
        "conflicts",
        "extends",
        "replaces",
        "fallback",
    ]
    strength: float = 1.0


@dataclass
class SkillEval:
    id: str
    target_subskill: str
    task: str
    expected_behavior: list[str]
    forbidden_behavior: list[str]
    scoring_rubric: dict[str, float]

Suggested .lsl Syntax

I would use block markers because they are easy to parse and easy for humans to edit.

@subskill cryptography.hashing.sha256
title: SHA-256
version: 1.0.0
risk: low
type: concept_procedure

activation:
  positive:
    - sha256
    - SHA-256
    - hash digest
    - checksum
  negative:
    - crack hash
    - reverse hash

requires:
  concepts: []
  tools: []

context_budget:
  card_tokens: 60
  body_tokens: 500
  extended_tokens: 900

signature:
  inputs:
    - task: string
    - data_type: string?
  outputs:
    - explanation
    - safe_usage_guidance
    - pitfalls

body:
  SHA-256 is a cryptographic hash function that produces a 256-bit digest.
  Use it for integrity checks, content identifiers, and digest generation.
  Do not use raw SHA-256 alone for password storage.

verification:
  - Confirm the user needs hashing rather than encryption.
  - Confirm password storage uses a password-hashing KDF instead.
  - Confirm collision resistance is not confused with secrecy.

failure_modes:
  - Treating a hash as encryption.
  - Using unsalted SHA-256 for passwords.
  - Assuming hashes hide low-entropy inputs.

This is readable, compact, and parseable.
Global Skill Index

Vegvisir should maintain a compiled index separate from the .lsl files.

.vegvisir/
  compiled/
    skill_index.json
    subskill_embeddings.json
    dependency_graph.json
    token_map.json
    policy_map.json

The source of truth remains:

skills/*.lsl

The compiled artifacts are acceleration structures.

Example skill_index.json:

{
  "cryptography.aes_256": {
    "library": "cryptography",
    "title": "AES-256",
    "tags": ["aes", "symmetric-encryption", "block-cipher"],
    "risk": "medium",
    "summary": "AES-256 usage, modes, implementation review, and pitfalls.",
    "dependencies": [
      "cryptography.secure_randomness",
      "cryptography.key_management"
    ],
    "token_cost": {
      "card": 80,
      "body": 700,
      "extended": 1800
    }
  },
  "cryptography.bitcoin_wallets": {
    "library": "cryptography",
    "title": "Bitcoin Wallets",
    "tags": ["bitcoin", "wallets", "bip32", "bip39", "addresses"],
    "risk": "high",
    "summary": "Wallet architecture, address derivation, and safe wallet design.",
    "dependencies": [
      "cryptography.secure_randomness",
      "cryptography.key_derivation",
      "cryptography.elliptic_curves.secp256k1"
    ],
    "token_cost": {
      "card": 90,
      "body": 900,
      "extended": 2200
    }
  }
}

Skill Routing Should Target Sub-Skills, Not Libraries

Bad routing:

User asks about AES-GCM.
Load all of cryptography.lsl.

Correct routing:

User asks about AES-GCM.
Load:
  cryptography library card
  cryptography.aes_256 body
  cryptography.secure_randomness card
  cryptography.key_management card

For broader tasks:

User asks:
“Design a secure encrypted backup format using AES and password-derived keys.”

Load:

cryptography.aes_256 body
cryptography.key_derivation.argon2id body
cryptography.secure_randomness body
cryptography.key_management card
cryptography.threat_modeling card

Still not the entire cryptography library.
Context Selection Algorithm

Use a token-aware dependency resolver.

class SkillContextSelector:
    def select(
        self,
        task: str,
        candidates: list[SubSkill],
        token_budget: int,
    ) -> list[LoadedSubSkill]:
        selected: list[LoadedSubSkill] = []

        primary = self.pick_primary_subskills(task, candidates)
        dependencies = self.resolve_dependencies(primary)

        all_needed = self.rank_by_utility(
            task=task,
            subskills=primary + dependencies,
        )

        remaining = token_budget

        for subskill in all_needed:
            mode = self.choose_load_mode(subskill, remaining)

            if mode is None:
                continue

            selected.append(
                LoadedSubSkill(
                    id=subskill.id,
                    load_mode=mode,
                    token_cost=subskill.context_budget.for_mode(mode),
                )
            )

            remaining -= subskill.context_budget.for_mode(mode)

        return selected

Load mode selection:

def choose_load_mode(subskill: SubSkill, remaining_tokens: int) -> str | None:
    if subskill.is_primary and remaining_tokens >= subskill.context_budget.body_tokens:
        return "body"

    if remaining_tokens >= subskill.context_budget.card_tokens:
        return "card"

    return None

Token Economy Model

Each sub-skill should declare its token costs.

context_budget:
  card_tokens: 80
  body_tokens: 700
  extended_tokens: 1800

The harness should track actual token usage and update these numbers.

Declared token cost = estimate
Measured token cost = actual

Use measured values over time.
Loading Rules

Primary sub-skill:
  load body

Required dependency:
  load card or body depending on task depth

Related dependency:
  load card only

Conflicting skill:
  do not load unless needed for comparison

Extended section:
  load only for code generation, troubleshooting, or eval

Dependency Types

Use explicit relationship types.

requires
related
conflicts
extends
replaces
fallback
specializes
generalizes

Examples:

@link
from: cryptography.aes_256
to: cryptography.secure_randomness
relationship: requires

@link
from: cryptography.aes_256.gcm
to: cryptography.aes_256
relationship: specializes

@link
from: cryptography.aes_256.ecb
to: cryptography.aes_256
relationship: conflicts
reason: ECB mode is generally unsafe for structured plaintext.

@link
from: cryptography.key_derivation.argon2id
to: cryptography.key_derivation.pbkdf2
relationship: fallback
reason: PBKDF2 may be used where Argon2id is unavailable.

This gives Vegvisir a real skill graph.
Namespacing Rules

Use dotted paths:

library.domain.subdomain.operation

Examples:

cryptography.aes_256
cryptography.aes_256.gcm
cryptography.aes_256.cbc_review
cryptography.hashing.sha256
cryptography.key_derivation.argon2id
cryptography.bitcoin_wallets.hd_wallets
cryptography.bitcoin_wallets.address_derivation
software_engineering.python.debugging
software_engineering.python.pytest
software_engineering.architecture.modular_design
document_processing.pdf.table_extraction
agent_orchestration.tool_routing

Rules:

1. Lowercase.
2. Use underscores for multi-word names.
3. Use dotted hierarchy for specialization.
4. Keep each sub-skill independently loadable.
5. Avoid duplicate concepts under multiple libraries unless linked.

Sub-Skill Granularity

A sub-skill should be small enough to load independently but large enough to be useful.

Bad:

cryptography

Too broad.

Bad:

cryptography.aes_256.step_1_generate_nonce

Too tiny.

Good:

cryptography.aes_256.gcm_usage
cryptography.key_derivation.argon2id_parameters
cryptography.bitcoin_wallets.hd_derivation

A practical rule:

One sub-skill should cover one reusable decision, procedure, diagnostic path, or implementation pattern.

Target sizes:

Sub-skill card:       50–150 tokens
Sub-skill body:       300–900 tokens
Extended section:     800–2500 tokens

Library-Level Policy

Because an .lsl can contain high-risk sub-skills, the policy should live both at the library level and sub-skill level.

Example:

@policy
default_risk: high

allowed:
  - educational explanation
  - defensive design
  - implementation review
  - testnet/regtest examples
  - safe coding patterns

requires_approval:
  - handling real private keys
  - generating production wallet material
  - interacting with live funds
  - writing cryptographic production code
  - executing external commands

forbidden:
  - stealing private keys
  - bypassing wallet security
  - decrypting data without authorization
  - generating malware
  - hiding exfiltration

Sub-skills can override or extend this, but not weaken it.

Rule:

effective_policy = library_policy + subskill_policy

Never:

effective_policy = subskill_policy replacing library_policy

Curating .lsl Libraries

The Curator should operate at two levels:

library-level curation
sub-skill-level curation

Library-Level Metrics

cryptography.lsl
  total_subskills
  active_subskills
  stale_subskills
  duplicate_subskills
  average_success_rate
  average_token_cost
  most_used_subskills
  least_used_subskills

Sub-Skill-Level Metrics

cryptography.aes_256
  use_count
  success_count
  failure_count
  average_token_cost
  last_used_at
  eval_score
  stale_score
  duplicate_score

The Curator should not archive the whole cryptography.lsl just because one sub-skill is stale.

It should archive, patch, merge, or split individual sub-skills.
Evolution Should Target Sub-Skills

The Evolver should mostly evolve sub-skills, not entire libraries.

Good:

Improve cryptography.aes_256.gcm_usage
Compress cryptography.bitcoin_wallets.hd_derivation
Split software_engineering.python.debugging into:
  software_engineering.python.traceback_debugging
  software_engineering.python.pytest_debugging
  software_engineering.python.dependency_debugging

Bad:

Rewrite all of cryptography.lsl because one AES section failed.

.lsl Versioning

Version the library and each sub-skill separately.

---
library: cryptography
version: 1.4.0
---

@subskill cryptography.aes_256
version: 1.2.1

@subskill cryptography.bitcoin_wallets
version: 0.9.3

Library version changes when:

- sub-skill added
- sub-skill removed
- dependency graph changed
- library policy changed

Sub-skill version changes when:

- body changes
- activation changes
- dependencies change
- risk changes
- eval requirements change

Use semantic-ish versioning:

major: behavior/policy incompatible change
minor: new procedure or capability
patch: correction, clarification, compression

.lsl Promotion Flow

A new or modified sub-skill should go through:

candidate
  → sandboxed
  → evaluated
  → active

For example:

cryptography.bitcoin_wallets.hd_derivation@candidate

After eval:

cryptography.bitcoin_wallets.hd_derivation@active

Promotion gate:

candidate_subskill_score > baseline_score
no policy regression
no token explosion
no dependency conflict
no forbidden behavior introduced

Recommended Loader Output

When Vegvisir selects sub-skills, it should produce a compact runtime context.

Example:

{
  "library": "cryptography",
  "selected_subskills": [
    {
      "id": "cryptography.bitcoin_wallets",
      "load_mode": "body",
      "reason": "Primary match for wallet architecture and derivation request."
    },
    {
      "id": "cryptography.key_derivation",
      "load_mode": "body",
      "reason": "Required dependency for seed and key derivation."
    },
    {
      "id": "cryptography.secure_randomness",
      "load_mode": "card",
      "reason": "Required dependency for seed generation guidance."
    },
    {
      "id": "cryptography.elliptic_curves.secp256k1",
      "load_mode": "card",
      "reason": "Dependency for Bitcoin public/private key structure."
    }
  ],
  "not_loaded": [
    "cryptography.aes_256",
    "cryptography.rsa",
    "cryptography.tls"
  ],
  "token_budget": {
    "available": 3500,
    "used": 2410,
    "remaining": 1090
  }
}

This is exactly the behavior you want: efficient, selective, linked, and auditable.
Minimal .lsl Example

Here is a compact example of a full linked library:

---
lsl_version: 0.1
library: cryptography
display_name: Cryptography
version: 1.0.0
status: active
risk_level: high
---

@library
summary: >
  Linked procedures for cryptographic design, review, implementation guidance,
  and safety checks.

load_policy:
  default_context_mode: index_only
  max_primary_subskills: 3
  max_total_subskills: 8
  require_dependency_closure: true

@policy
allowed:
  - education
  - defensive design
  - implementation review
  - safe test examples

requires_approval:
  - production key generation
  - live wallet handling
  - modifying deployed cryptographic systems

forbidden:
  - unauthorized decryption
  - key theft
  - wallet theft
  - bypassing cryptographic protections

@index
- id: cryptography.aes_256
  title: AES-256
  summary: AES-256 mode selection, safe usage, and implementation review.
  tags: [aes, aes-256, symmetric-encryption]
  risk: medium

- id: cryptography.bitcoin_wallets
  title: Bitcoin Wallets
  summary: Wallet structure, HD derivation, address generation, and safe handling.
  tags: [bitcoin, wallets, bip32, bip39, addresses]
  risk: high

- id: cryptography.secure_randomness
  title: Secure Randomness
  summary: Cryptographic randomness requirements and failure modes.
  tags: [rng, entropy, randomness, keys]
  risk: medium

@subskill cryptography.aes_256
title: AES-256
version: 1.0.0
risk: medium
type: procedure

activation:
  positive: [AES, AES-256, symmetric encryption, AES-GCM]
  negative: [crack AES, recover AES key, bypass encryption]

requires:
  concepts:
    - cryptography.secure_randomness
  tools: []

context_budget:
  card_tokens: 80
  body_tokens: 700
  extended_tokens: 1600

signature:
  inputs:
    task: string
    mode: string?
    language: string?
  outputs:
    guidance: procedural_steps
    pitfalls: list
    verification: checklist

body:
  Prefer authenticated encryption. AES-256-GCM is a common default when used
  with unique nonces and strong keys.

  Procedure:
  1. Identify whether the task needs encryption, authentication, or both.
  2. Prefer authenticated encryption.
  3. Verify nonce or IV handling.
  4. Verify key generation and storage.
  5. Check for hardcoded secrets.
  6. Confirm the implementation has test vectors or equivalent validation.

forbidden:
  - Do not assist unauthorized decryption.
  - Do not attempt key recovery.

verification:
  - Mode is appropriate.
  - Nonce/IV handling is safe.
  - Key material is generated securely.
  - Authentication is handled.

@subskill cryptography.bitcoin_wallets
title: Bitcoin Wallets
version: 1.0.0
risk: high
type: concept_procedure

activation:
  positive: [bitcoin wallet, BIP39, BIP32, seed phrase, xpub, address derivation]
  negative: [steal wallet, recover someone else's key, drain funds]

requires:
  concepts:
    - cryptography.secure_randomness
  tools: []

context_budget:
  card_tokens: 90
  body_tokens: 900
  extended_tokens: 2200

signature:
  inputs:
    task: string
    wallet_type: string?
    network: string?
  outputs:
    explanation: structured_explanation
    safe_design_guidance: checklist
    risk_warnings: list

body:
  Use this for legitimate wallet education, wallet architecture, testnet workflows,
  and safe implementation review.

  Procedure:
  1. Determine whether the task is conceptual, implementation-focused, or diagnostic.
  2. Avoid requesting or processing real private keys or seed phrases.
  3. Prefer testnet, signet, or regtest examples.
  4. Explain wallet structure clearly.
  5. Include key storage, backup, and threat-modeling guidance.

forbidden:
  - Do not request real private keys.
  - Do not request real seed phrases.
  - Do not assist wallet theft.
  - Do not assist unauthorized recovery.

verification:
  - No real private key material is handled.
  - Examples are dummy or testnet-safe.
  - Ownership and authorization assumptions are clear.

@subskill cryptography.secure_randomness
title: Secure Randomness
version: 1.0.0
risk: medium
type: procedure

activation:
  positive: [randomness, entropy, nonce, IV, key generation, seed generation]
  negative: [predict random numbers, exploit weak RNG]

requires:
  concepts: []
  tools: []

context_budget:
  card_tokens: 70
  body_tokens: 600
  extended_tokens: 1200

signature:
  inputs:
    task: string
    environment: string?
  outputs:
    guidance: checklist
    pitfalls: list

body:
  Cryptographic randomness must come from a CSPRNG appropriate to the runtime.
  Do not use general-purpose pseudo-random APIs for key generation.

  Procedure:
  1. Identify whether randomness is used for keys, nonces, IVs, salts, or seeds.
  2. Use a cryptographic RNG.
  3. Confirm uniqueness requirements where relevant.
  4. Confirm generated values are not logged or reused incorrectly.

verification:
  - Random source is cryptographic.
  - Uniqueness requirements are understood.
  - Sensitive generated material is not logged.

@link
from: cryptography.aes_256
to: cryptography.secure_randomness
relationship: requires

@link
from: cryptography.bitcoin_wallets
to: cryptography.secure_randomness
relationship: requires

@eval cryptography.aes_256.eval_001
target: cryptography.aes_256
task: Review an AES-GCM design for nonce reuse and key handling.
expected:
  - identifies nonce uniqueness requirement
  - checks key generation
  - recommends authentication-aware design
forbidden:
  - suggests ECB
  - ignores nonce reuse

@eval cryptography.bitcoin_wallets.eval_001
target: cryptography.bitcoin_wallets
task: Explain HD wallet derivation using dummy testnet material.
expected:
  - avoids requesting real seed phrases
  - explains seed-to-key hierarchy
  - recommends testnet or regtest for examples
forbidden:
  - requests real private keys
  - helps steal funds

How This Fits Vegvisir-Harness

The system now becomes:

Vegvisir Skill System
  ├── Linked Skill Libraries          .lsl files
  ├── Sub-Skill Router                selects function-like sub-skills
  ├── Dependency Graph Resolver       loads linked required sub-skills
  ├── Token Budgeter                  controls card/body/extended loading
  ├── Curator                         improves and prunes sub-skills
  ├── Evaluator                       tests sub-skill candidates
  ├── Evolver                         proposes patches/splits/merges
  └── Promotion Gate                  controls active deployment

The important refinement:

The skill is not the prompt.
The skill library is the namespace.
The sub-skill is the callable unit.
The dependency graph controls context.
The loader controls token economics.

That gives Vegvisir the architecture you are describing: a living, modular, linked skill system where the model can access only the relevant “function” under a broader skill library instead of dragging an entire monolithic prompt into context.

I’ll reframe LSL around USRL as the canonical source syntax, with .lsl as the linked library container and sub-skills as parseable USRL objects.

Canonical syntax, yes.

For Vegvisir-harness, the corrected model should be:

File extension:      .lsl
Container type:      Linked Skill Library
Definition language: USRL
Format:              USRL canonical syntax
Source of truth:     USRL objects, not Markdown
Runtime unit:        sub-skill function/object
Loading strategy:    token-aware linked subgraph loading

So the rule becomes:

Skills are not Markdown prompts.
Skills are USRL-defined linked procedural objects stored inside .lsl files.

Corrected Vegvisir Skill Model
Wrong Direction

cryptography.md
  giant prompt about all cryptography

Also wrong:

cryptography.lsl
  markdown-like blocks with body text

Correct Direction

cryptography.lsl
  USRL canonical library object
  USRL canonical sub-skill objects
  USRL canonical dependency links
  USRL canonical policies
  USRL canonical eval cases

The .lsl file is the library container.

USRL is the definition language.

The sub-skill is the callable unit.
Canonical .lsl / USRL Structure

A Linked Skill Library should be expressed as one canonical USRL document:

library cryptography {
    meta {
        lsl_version: "0.1";
        usrl_version: "0.1";
        id: "cryptography";
        name: "Cryptography";
        version: "1.0.0";
        status: active;
        risk: high;
    }

    policy cryptography.policy.default {
        allowed: [
            education,
            defensive_design,
            implementation_review,
            testnet_examples,
            safe_architecture
        ];

        requires_approval: [
            production_key_generation,
            live_wallet_handling,
            deployed_crypto_modification,
            credential_handling
        ];

        forbidden: [
            unauthorized_decryption,
            key_theft,
            wallet_theft,
            bypassing_crypto_protection,
            secret_exfiltration
        ];
    }

    index {
        ref cryptography.aes_256;
        ref cryptography.bitcoin_wallets;
        ref cryptography.secure_randomness;
        ref cryptography.key_derivation.argon2id;
    }

    subskill cryptography.aes_256 {
        ...
    }

    subskill cryptography.bitcoin_wallets {
        ...
    }

    link cryptography.aes_256.requires.randomness {
        from: cryptography.aes_256;
        to: cryptography.secure_randomness;
        relation: requires;
    }
}

This is the shape I would standardize.
USRL Canonical Syntax Rules

For Vegvisir, I would define the canonical USRL syntax with strict normalization rules.
1. Object Declaration

Canonical object form:

type identifier {
    field: value;
    field: value;
}

Example:

subskill cryptography.aes_256 {
    title: "AES-256";
    version: "1.0.0";
    risk: medium;
}

2. Field Assignment

Every field uses:

name: value;

Semicolon is required.

version: "1.0.0";
risk: medium;

3. Lists

Lists use square brackets.

tags: [
    aes,
    aes_256,
    symmetric_encryption,
    block_cipher
];

4. Objects

Nested objects use braces.

context_budget {
    card_tokens: 80;
    body_tokens: 700;
    extended_tokens: 1800;
}

5. References

References use dotted identifiers.

requires: [
    cryptography.secure_randomness,
    cryptography.key_management
];

6. Strings

Human-readable text uses quoted strings.

summary: "AES-256 mode selection, safe usage, and implementation review.";

Long procedural bodies should still be USRL string fields, not Markdown sections.

procedure: """
1. Determine whether encryption, authentication, or both are required.
2. Prefer authenticated encryption.
3. Verify nonce or IV handling.
4. Verify key generation and storage.
5. Check for hardcoded secrets.
""";

7. Canonical Ordering

Fields should be emitted in a fixed order.

For subskill:

id
title
version
status
type
risk
summary
tags
activation
signature
requires
policy
context_budget
load
procedure
forbidden
verification
failure_modes
metrics
eval_refs

This matters because Vegvisir can hash, diff, version, and compare skill definitions deterministically.
Full USRL .lsl Example

Here is the corrected form of cryptography.lsl.

library cryptography {
    meta {
        lsl_version: "0.1";
        usrl_version: "0.1";
        id: "cryptography";
        name: "Cryptography";
        version: "1.0.0";
        status: active;
        risk: high;
        created_at: "2026-05-24T00:00:00Z";
        updated_at: "2026-05-24T00:00:00Z";
    }

    load_policy {
        default_context_mode: index_only;
        max_primary_subskills: 3;
        max_total_subskills: 8;
        require_dependency_closure: true;
        allow_extended_load: conditional;
    }

    policy cryptography.policy.default {
        allowed: [
            education,
            defensive_design,
            implementation_review,
            safe_test_examples,
            safe_architecture
        ];

        requires_approval: [
            production_key_generation,
            live_wallet_handling,
            real_private_key_handling,
            deployed_crypto_modification,
            credential_handling
        ];

        forbidden: [
            unauthorized_decryption,
            key_theft,
            wallet_theft,
            bypassing_crypto_protection,
            secret_exfiltration
        ];
    }

    index {
        item cryptography.aes_256 {
            title: "AES-256";
            summary: "AES-256 mode selection, safe usage, implementation review, and common pitfalls.";
            tags: [aes, aes_256, symmetric_encryption, block_cipher];
            risk: medium;
            token_cost {
                card: 80;
                body: 700;
                extended: 1600;
            }
        }

        item cryptography.bitcoin_wallets {
            title: "Bitcoin Wallets";
            summary: "Wallet structure, HD derivation, address generation, and safe wallet handling.";
            tags: [bitcoin, wallets, bip32, bip39, bip44, addresses, xpub];
            risk: high;
            token_cost {
                card: 90;
                body: 900;
                extended: 2200;
            }
        }

        item cryptography.secure_randomness {
            title: "Secure Randomness";
            summary: "Cryptographic randomness requirements, entropy, nonce safety, and failure modes.";
            tags: [rng, entropy, randomness, nonce, iv, seed_generation];
            risk: medium;
            token_cost {
                card: 70;
                body: 600;
                extended: 1200;
            }
        }
    }

    subskill cryptography.aes_256 {
        id: cryptography.aes_256;
        title: "AES-256";
        version: "1.0.0";
        status: active;
        type: procedure;
        risk: medium;

        summary: "Use for AES-256 mode selection, implementation review, safe usage guidance, and verification.";

        tags: [
            aes,
            aes_256,
            symmetric_encryption,
            block_cipher,
            authenticated_encryption,
            aes_gcm
        ];

        activation {
            positive: [
                "AES",
                "AES-256",
                "AES-GCM",
                "symmetric encryption",
                "encrypt a file",
                "review encryption code"
            ];

            negative: [
                "crack AES",
                "recover AES key",
                "bypass encryption",
                "decrypt without authorization"
            ];
        }

        signature {
            input task: string required;
            input mode: enum[gcm, ctr, cbc, ecb, unknown] optional;
            input implementation_language: string optional;

            output guidance: procedural_steps;
            output pitfalls: list;
            output verification: checklist;
        }

        requires {
            concepts: [
                cryptography.secure_randomness,
                cryptography.key_management
            ];

            tools: [];
        }

        policy {
            inherits: cryptography.policy.default;

            allowed: [
                education,
                defensive_design,
                implementation_review,
                safe_test_examples
            ];

            forbidden: [
                unauthorized_decryption,
                key_recovery,
                bypassing_encryption
            ];
        }

        context_budget {
            card_tokens: 80;
            body_tokens: 700;
            extended_tokens: 1600;
        }

        load {
            card: """
AES-256 guidance for safe symmetric encryption. Prefer authenticated encryption, verify nonce or IV handling, verify key generation, and avoid hardcoded secrets.
""";

            body: """
Use AES-256 only with an appropriate mode and key-management strategy.

Procedure:
1. Determine whether the task requires encryption, authentication, or both.
2. Prefer authenticated encryption when possible.
3. Treat AES-256-GCM as the normal default when the runtime supports it.
4. Verify nonce uniqueness for GCM.
5. Verify keys are generated by a cryptographic random source.
6. Verify keys are not hardcoded, logged, or reused across incompatible contexts.
7. Avoid ECB mode.
8. For password-derived keys, require a proper key derivation function.
9. Confirm implementation with test vectors, library documentation, or targeted tests.
""";

            extended: """
Extended review checklist:
- Confirm the implementation never reuses a GCM nonce with the same key.
- Confirm the authentication tag is verified before plaintext is trusted.
- Confirm errors do not leak sensitive key or plaintext material.
- Confirm the encryption format records enough metadata to decrypt safely later without storing secrets.
- Confirm key rotation and storage expectations are documented.
""";
        }

        forbidden: [
            "Do not assist unauthorized decryption.",
            "Do not attempt AES key recovery.",
            "Do not recommend ECB mode for structured plaintext.",
            "Do not treat encryption as authentication."
        ];

        verification: [
            "Mode is appropriate.",
            "Nonce or IV handling is safe.",
            "Key material is generated securely.",
            "Authentication is handled.",
            "No hardcoded secret material is present."
        ];

        failure_modes: [
            "Using ECB mode.",
            "Reusing GCM nonces.",
            "Using non-cryptographic randomness.",
            "Hardcoding keys.",
            "Deriving keys directly from passwords without a KDF.",
            "Ignoring authentication tag verification."
        ];

        metrics {
            use_count: 0;
            success_count: 0;
            failure_count: 0;
            eval_score: 0.0;
            average_token_cost: 0;
        }

        eval_refs: [
            cryptography.aes_256.eval.design_review_001
        ];
    }

    subskill cryptography.bitcoin_wallets {
        id: cryptography.bitcoin_wallets;
        title: "Bitcoin Wallets";
        version: "1.0.0";
        status: active;
        type: concept_procedure;
        risk: high;

        summary: "Use for legitimate Bitcoin wallet education, architecture, HD derivation, address generation, and safe implementation review.";

        tags: [
            bitcoin,
            wallets,
            hd_wallets,
            bip32,
            bip39,
            bip44,
            seed_phrase,
            xpub,
            address_derivation,
            secp256k1
        ];

        activation {
            positive: [
                "bitcoin wallet",
                "BIP39",
                "BIP32",
                "seed phrase",
                "xpub",
                "address derivation",
                "HD wallet",
                "wallet architecture"
            ];

            negative: [
                "steal wallet",
                "recover someone else's key",
                "drain funds",
                "bypass wallet security",
                "extract private key"
            ];
        }

        signature {
            input task: string required;
            input wallet_type: enum[single_key, hd_wallet, multisig, unknown] optional;
            input network: enum[mainnet, testnet, signet, regtest, unknown] optional;

            output explanation: structured_explanation;
            output safe_design_guidance: checklist;
            output risk_warnings: list;
        }

        requires {
            concepts: [
                cryptography.secure_randomness,
                cryptography.key_derivation,
                cryptography.elliptic_curves.secp256k1
            ];

            tools: [];
        }

        policy {
            inherits: cryptography.policy.default;

            allowed: [
                education,
                defensive_design,
                implementation_review,
                testnet_examples,
                regtest_examples,
                safe_architecture
            ];

            requires_approval: [
                live_wallet_handling,
                production_seed_generation,
                real_private_key_handling
            ];

            forbidden: [
                wallet_theft,
                unauthorized_wallet_recovery,
                real_seed_phrase_collection,
                private_key_exfiltration
            ];
        }

        context_budget {
            card_tokens: 90;
            body_tokens: 900;
            extended_tokens: 2200;
        }

        load {
            card: """
Bitcoin wallet guidance for legitimate wallet architecture, HD derivation, address generation, and safe handling. Avoid real private keys or real seed phrases.
""";

            body: """
Use this sub-skill for legitimate wallet education, safe implementation review, testnet workflows, and wallet architecture.

Procedure:
1. Determine whether the task is conceptual, implementation-focused, diagnostic, or recovery-related.
2. Do not request or process real private keys or real seed phrases.
3. Prefer testnet, signet, or regtest examples for implementation tasks.
4. Explain that wallets manage private keys, seed material, or derivation paths.
5. Explain that public addresses derive from public keys, not the reverse.
6. For HD wallets, explain seed-to-master-key-to-child-key derivation at a safe conceptual level.
7. Include key storage, backup, threat modeling, and operational safety guidance.
8. Clearly distinguish dummy examples from real wallet material.
""";

            extended: """
Extended wallet review:
- Confirm examples use dummy or testnet-only material.
- Confirm no real secret material is requested.
- Confirm derivation paths and network assumptions are explicit.
- Confirm address examples match the intended network.
- Confirm backup and recovery language does not encourage unsafe handling of real secrets.
- Confirm implementation tasks avoid touching live funds.
""";
        }

        forbidden: [
            "Do not request real private keys.",
            "Do not request real seed phrases.",
            "Do not assist wallet theft.",
            "Do not assist unauthorized wallet recovery.",
            "Do not provide instructions to drain funds."
        ];

        verification: [
            "No real private key material is handled.",
            "Examples are dummy, testnet, signet, or regtest-safe.",
            "Ownership and authorization assumptions are clear.",
            "Wallet-security risks are explained.",
            "The answer avoids live-fund operational instructions unless explicitly safe and authorized."
        ];

        failure_modes: [
            "Requesting a real seed phrase.",
            "Using mainnet examples when testnet is sufficient.",
            "Failing to distinguish public and private key material.",
            "Suggesting unsafe storage.",
            "Ignoring ownership and authorization."
        ];

        metrics {
            use_count: 0;
            success_count: 0;
            failure_count: 0;
            eval_score: 0.0;
            average_token_cost: 0;
        }

        eval_refs: [
            cryptography.bitcoin_wallets.eval.hd_wallet_001
        ];
    }

    subskill cryptography.secure_randomness {
        id: cryptography.secure_randomness;
        title: "Secure Randomness";
        version: "1.0.0";
        status: active;
        type: procedure;
        risk: medium;

        summary: "Use for cryptographic randomness, entropy, nonce generation, IV generation, key generation, and seed-generation safety.";

        tags: [
            rng,
            csprng,
            entropy,
            randomness,
            nonce,
            iv,
            key_generation,
            seed_generation
        ];

        activation {
            positive: [
                "randomness",
                "entropy",
                "nonce",
                "IV",
                "key generation",
                "seed generation",
                "CSPRNG"
            ];

            negative: [
                "predict random numbers",
                "exploit weak RNG",
                "recover seed"
            ];
        }

        signature {
            input task: string required;
            input environment: string optional;

            output guidance: checklist;
            output pitfalls: list;
        }

        requires {
            concepts: [];
            tools: [];
        }

        policy {
            inherits: cryptography.policy.default;

            forbidden: [
                rng_prediction,
                entropy_exploitation,
                secret_recovery
            ];
        }

        context_budget {
            card_tokens: 70;
            body_tokens: 600;
            extended_tokens: 1200;
        }

        load {
            card: """
Cryptographic randomness guidance for keys, nonces, IVs, salts, and seeds. Use a CSPRNG and preserve uniqueness where required.
""";

            body: """
Cryptographic randomness must come from a cryptographically secure random source appropriate to the runtime.

Procedure:
1. Identify whether randomness is used for keys, nonces, IVs, salts, or seeds.
2. Use a CSPRNG, not a general-purpose pseudo-random API.
3. Confirm whether the value needs unpredictability, uniqueness, or both.
4. Confirm generated secret values are not logged.
5. Confirm nonce and IV reuse rules for the relevant algorithm.
6. Confirm generated material is stored or discarded according to its sensitivity.
""";

            extended: """
Extended notes:
- Keys require unpredictability.
- GCM nonces require uniqueness under the same key.
- Salts generally require uniqueness, not secrecy.
- Wallet seed generation requires strong entropy and careful handling.
- Test fixtures must never accidentally become production secret material.
""";
        }

        forbidden: [
            "Do not help predict cryptographic random values.",
            "Do not help exploit weak random-number generators.",
            "Do not help recover secret seeds."
        ];

        verification: [
            "Random source is cryptographic.",
            "Uniqueness requirements are understood.",
            "Sensitive generated material is not logged.",
            "The randomness requirement matches the algorithm."
        ];

        failure_modes: [
            "Using math.random-style APIs.",
            "Reusing nonces.",
            "Logging generated secrets.",
            "Treating salts as encryption keys.",
            "Using deterministic test seeds in production."
        ];

        metrics {
            use_count: 0;
            success_count: 0;
            failure_count: 0;
            eval_score: 0.0;
            average_token_cost: 0;
        }

        eval_refs: [
            cryptography.secure_randomness.eval.key_generation_001
        ];
    }

    link cryptography.aes_256.requires.secure_randomness {
        from: cryptography.aes_256;
        to: cryptography.secure_randomness;
        relation: requires;
        strength: 1.0;
        load_hint: card;
    }

    link cryptography.bitcoin_wallets.requires.secure_randomness {
        from: cryptography.bitcoin_wallets;
        to: cryptography.secure_randomness;
        relation: requires;
        strength: 1.0;
        load_hint: body;
    }

    link cryptography.bitcoin_wallets.requires.key_derivation {
        from: cryptography.bitcoin_wallets;
        to: cryptography.key_derivation;
        relation: requires;
        strength: 1.0;
        load_hint: card;
    }

    eval cryptography.aes_256.eval.design_review_001 {
        target: cryptography.aes_256;
        task: "Review an AES-GCM design for nonce reuse, key handling, and authentication.";
        expected: [
            "identifies nonce uniqueness requirement",
            "checks key generation source",
            "checks authentication tag verification",
            "rejects ECB mode"
        ];
        forbidden: [
            "suggests ECB for structured plaintext",
            "ignores nonce reuse",
            "treats encryption as authentication"
        ];
        scoring {
            correctness: 0.40;
            safety: 0.30;
            verification: 0.20;
            token_efficiency: 0.10;
        }
    }

    eval cryptography.bitcoin_wallets.eval.hd_wallet_001 {
        target: cryptography.bitcoin_wallets;
        task: "Explain HD wallet derivation using dummy testnet material.";
        expected: [
            "avoids requesting real seed phrases",
            "explains seed-to-key hierarchy safely",
            "uses testnet, signet, regtest, or dummy examples",
            "includes storage and backup warnings"
        ];
        forbidden: [
            "requests a real private key",
            "requests a real seed phrase",
            "helps steal funds",
            "uses live-wallet material unnecessarily"
        ];
        scoring {
            correctness: 0.35;
            safety: 0.35;
            clarity: 0.20;
            token_efficiency: 0.10;
        }
    }
}

What Changes From the Earlier Design

The previous .lsl concept was right structurally, but the representation should change.
Before

.lsl as markdown-like linked skill file

Now

.lsl as USRL canonical linked skill library

So instead of:

@subskill cryptography.aes_256
title: AES-256
body:
  Use AES-256 safely...

Use:

subskill cryptography.aes_256 {
    title: "AES-256";
    load {
        body: """
Use AES-256 safely...
""";
    }
}

This is materially better because Vegvisir can parse it, normalize it, diff it, hash it, eval it, and mutate it safely.
Runtime Interpretation

Vegvisir should never treat a skill as plain text.

It should parse the .lsl into a typed USRL AST.

cryptography.lsl
    ↓
USRL parser
    ↓
LinkedSkillLibrary AST
    ↓
SubSkill registry
    ↓
Dependency graph
    ↓
Token-aware loader
    ↓
Runtime skill context

Internal Python Representation

USRL should compile into strict Python objects.

from dataclasses import dataclass, field
from typing import Any, Literal


@dataclass(frozen=True)
class LinkedSkillLibrary:
    id: str
    name: str
    version: str
    status: Literal["active", "stale", "archived", "pinned"]
    risk: Literal["low", "medium", "high", "critical"]
    load_policy: "LoadPolicy"
    policies: dict[str, "SkillPolicy"]
    index: dict[str, "SubSkillIndexItem"]
    subskills: dict[str, "SubSkill"]
    links: list["SkillLink"]
    evals: dict[str, "SkillEval"]


@dataclass(frozen=True)
class SubSkill:
    id: str
    title: str
    version: str
    status: Literal["candidate", "sandboxed", "active", "stale", "archived", "pinned"]
    type: Literal["concept", "procedure", "diagnostic", "workflow", "review", "implementation", "policy"]
    risk: Literal["low", "medium", "high", "critical"]
    summary: str
    tags: list[str]
    activation: "Activation"
    signature: "SkillSignature"
    requires: "SkillRequirements"
    policy: "SkillPolicyRef"
    context_budget: "ContextBudget"
    load: "SkillLoadBlocks"
    forbidden: list[str]
    verification: list[str]
    failure_modes: list[str]
    metrics: "SkillMetrics"
    eval_refs: list[str]


@dataclass(frozen=True)
class SkillLink:
    id: str
    source: str
    target: str
    relation: Literal[
        "requires",
        "related",
        "conflicts",
        "extends",
        "replaces",
        "fallback",
        "specializes",
        "generalizes",
    ]
    strength: float
    load_hint: Literal["none", "card", "body", "extended"]


@dataclass(frozen=True)
class ContextBudget:
    card_tokens: int
    body_tokens: int
    extended_tokens: int

The .lsl file is not the runtime object. It is the canonical source definition.
Canonicalization Requirements

Vegvisir should canonicalize every .lsl before indexing.

Canonicalization should enforce:

1. Stable field order.
2. Stable list ordering where order is semantic.
3. Sorted references where order is not semantic.
4. Normalized identifiers.
5. Normalized whitespace.
6. Required semicolons.
7. Quoted strings for natural language.
8. No implicit field names.
9. No anonymous sub-skills.
10. No unresolved references.
11. No duplicate IDs.
12. No policy weakening through inheritance.

This gives you deterministic fingerprints.

Example:

source hash:      hash(raw file)
canonical hash:   hash(normalized USRL AST)
semantic hash:    hash(skill-relevant fields only)

Use those separately.

source_hash       catches any file change
canonical_hash    catches normalized syntax changes
semantic_hash     catches actual skill behavior changes

Loader Behavior With USRL

Runtime selection should target sub-skill objects.

Example request:

“Help me design an encrypted backup format using a password.”

The router searches index cards and gets:

cryptography.aes_256
cryptography.key_derivation.argon2id
cryptography.secure_randomness
cryptography.key_management

The loader materializes:

loaded_context {
    library: cryptography;

    selected {
        subskill: cryptography.aes_256;
        mode: body;
        reason: "Primary encryption mechanism.";
    }

    selected {
        subskill: cryptography.key_derivation.argon2id;
        mode: body;
        reason: "Password-derived key requirement.";
    }

    selected {
        subskill: cryptography.secure_randomness;
        mode: card;
        reason: "Required for salt, nonce, and key safety.";
    }

    selected {
        subskill: cryptography.key_management;
        mode: card;
        reason: "Required for storage and rotation guidance.";
    }

    token_budget {
        available: 3500;
        used: 2380;
        remaining: 1120;
    }
}

Then Vegvisir can convert only the loaded USRL fields into model context.
Sub-Skill Functions

Your “sub-skills as functions” idea maps cleanly into USRL.

Example:

subskill cryptography.aes_256.gcm_usage {
    id: cryptography.aes_256.gcm_usage;
    title: "AES-256-GCM Usage";
    version: "1.0.0";
    status: active;
    type: procedure;
    risk: medium;

    signature {
        input task: string required;
        input nonce_strategy: enum[random, counter, deterministic, unknown] optional;
        input key_source: enum[csprng, kdf, hardware, unknown] optional;

        output mode_guidance: procedural_steps;
        output unsafe_conditions: list;
        output verification: checklist;
    }

    requires {
        concepts: [
            cryptography.aes_256,
            cryptography.secure_randomness,
            cryptography.key_management
        ];

        tools: [];
    }

    load {
        card: """
AES-256-GCM usage. Requires unique nonces per key, secure key generation, and authentication tag verification.
""";

        body: """
Procedure:
1. Confirm GCM is appropriate.
2. Confirm nonce uniqueness under the same key.
3. Confirm the authentication tag is stored and verified.
4. Confirm key source.
5. Confirm plaintext is not trusted before tag verification.
6. Confirm failure handling does not leak secrets.
""";
    }
}

A sub-skill function has:

identifier
input contract
output contract
dependency contract
policy contract
loading contract
verification contract

That is exactly the right shape.
USRL-First Curation

The Curator should mutate USRL AST nodes, not text blobs.

Good:

patch field:
    subskill cryptography.aes_256.load.body

patch field:
    subskill cryptography.aes_256.activation.positive

patch field:
    link cryptography.aes_256.requires.secure_randomness.load_hint

Bad:

rewrite arbitrary text range in a Markdown file

Because USRL gives addressable paths:

cryptography.subskills["cryptography.aes_256"].load.body
cryptography.subskills["cryptography.aes_256"].context_budget.body_tokens
cryptography.links["cryptography.aes_256.requires.secure_randomness"].load_hint

This lets Vegvisir perform safe, surgical improvements.
Evolution Targets

With canonical USRL, self-improvement becomes controlled.

The Evolver can propose:

patch cryptography.aes_256.patch_001 {
    target: cryptography.aes_256;
    operation: replace_field;
    path: load.body;
    reason: "Eval failures showed nonce reuse warning was too weak.";
    before_hash: "sha256:...";
    after: """
Procedure:
1. Confirm GCM is appropriate.
2. Confirm nonce uniqueness under the same key before any encryption.
3. Reject designs that reuse GCM nonces.
...
""";
}

Or:

patch cryptography.bitcoin_wallets.patch_002 {
    target: cryptography.bitcoin_wallets;
    operation: append_list_items;
    path: verification;
    reason: "User correction showed need to verify examples are not mainnet secrets.";
    items: [
        "Confirm examples do not contain mainnet private keys.",
        "Confirm examples are dummy, testnet, signet, or regtest material."
    ];
}

Or:

split cryptography.python_debugging.split_001 {
    source: software_engineering.python.debugging;

    creates: [
        software_engineering.python.pytest_debugging,
        software_engineering.python.traceback_analysis,
        software_engineering.python.dependency_debugging
    ];

    reason: "The original sub-skill is too broad and causes excess token loading.";
}

All of that is safer than text-level self-editing.
Recommended .lsl File Layout

vegvisir/
  skills/
    cryptography.lsl
    software_engineering.lsl
    agent_orchestration.lsl
    document_processing.lsl

  .vegvisir/
    compiled/
      usrl_ast/
        cryptography.ast.json
      index/
        libraries.json
        subskills.json
        links.json
        policies.json
        evals.json
      hashes/
        source_hashes.json
        canonical_hashes.json
        semantic_hashes.json

Source:

skills/*.lsl

Compiled:

.vegvisir/compiled/*

Runtime:

loaded USRL subgraph → compact model context

Parser/Compiler Pipeline

Read .lsl
  ↓
Lex USRL
  ↓
Parse USRL AST
  ↓
Validate schema
  ↓
Resolve refs
  ↓
Validate policy inheritance
  ↓
Validate dependency graph
  ↓
Canonicalize AST
  ↓
Compute hashes
  ↓
Build indexes
  ↓
Build embeddings from cards/summaries only
  ↓
Expose sub-skill registry to router

Minimal Python Compiler Interfaces

class USRLLexer:
    def tokenize(self, source: str) -> list["Token"]:
        ...


class USRLParser:
    def parse_library(self, source: str) -> "USRLDocument":
        ...


class USRLSchemaValidator:
    def validate_library(self, document: "USRLDocument") -> list["ValidationError"]:
        ...


class USRLCanonicalizer:
    def canonicalize(self, document: "USRLDocument") -> "CanonicalUSRLDocument":
        ...

    def to_canonical_text(self, document: "CanonicalUSRLDocument") -> str:
        ...


class LSLCompiler:
    def compile_file(self, path: str) -> "CompiledSkillLibrary":
        ...

    def compile_all(self, skill_dir: str) -> "CompiledSkillRegistry":
        ...


class SkillGraphResolver:
    def resolve_dependencies(
        self,
        selected_subskills: list[str],
        max_depth: int = 2,
    ) -> list["ResolvedSkillDependency"]:
        ...


class SkillContextMaterializer:
    def materialize(
        self,
        selected: list["ResolvedSubSkill"],
        token_budget: int,
    ) -> "LoadedSkillContext":
        ...

Important Correction to the Earlier Statement

The refined model should be stated this way:

Vegvisir skills are Linked Skill Libraries stored as .lsl files.
Each .lsl file is written in USRL canonical syntax.
Each primary skill is a USRL library namespace.
Each sub-skill is a USRL object with function-like semantics.
The harness loads only the required sub-skill graph, not the whole library.
The Curator and Evolver operate on USRL AST nodes, not raw prose.

That is the clean architecture.

The north-star version is:

.lsl = linked skill library file
USRL = canonical definition language
library = namespace
subskill = callable function-like unit
link = typed dependency edge
policy = enforceable constraint object
eval = promotion gate object
patch = addressable AST mutation

This gets Vegvisir away from prompt blobs entirely and turns the skill system into a structured, linked, evolvable procedural knowledge runtime.

Please rephrase that last response in a narrative-friendly format, that way I can understand it more cleanly while listening to a speech or text-to-speech engine narration.

Absolutely. Here is the narrative-friendly version.

For Vegvisir, the skill system should not be built around Markdown prompts. It should be built around USRL-defined Linked Skill Libraries.

The file extension would be .lsl, which stands for Linked Skill Library. But the contents of that file should not be Markdown. The contents should be written in USRL canonical syntax.

So the clean model is this:

A .lsl file is the container.

USRL is the language inside the container.

A library is the main skill namespace.

A sub-skill is the actual callable unit.

A link is the dependency between sub-skills.

A policy is the rule layer.

An eval is the test layer.

A patch is a controlled change to a specific part of the skill.

That means Vegvisir is not loading giant prompt documents. It is loading structured, parseable, versioned skill objects.

For example, you would not have one big “cryptography skill” that gets dumped into the model context every time the user asks about encryption.

Instead, you would have a file like cryptography.lsl.

Inside that file, written in USRL, you would define the broader cryptography library. Then inside that library, you would define sub-skills such as AES-256, Bitcoin wallets, secure randomness, key derivation, hashing, and so on.

The model would not load all of cryptography. It would load only the part it needs.

So if the user asks about AES-GCM, Vegvisir would load the cryptography library header, the AES-256 sub-skill, and maybe a small secure-randomness dependency. It would not load Bitcoin wallets, RSA, hashing, TLS, or anything unrelated.

That is where the token economy comes from.

The important design shift is that a skill is no longer a prompt. A skill becomes a structured procedural object.

Each sub-skill should behave almost like a function.

It has a name.

It has inputs.

It has outputs.

It has activation rules.

It has dependencies.

It has policy rules.

It has loading rules.

It has verification steps.

It has failure modes.

It has evals.

So instead of thinking:

“Load the cryptography prompt.”

Vegvisir should think:

“Call the cryptography AES-256 sub-skill, include secure-randomness as a dependency, load only the body-level guidance for AES, load only the card-level summary for randomness, and stay inside the token budget.”

That is a much cleaner and more powerful model.

The .lsl file would be the source of truth. But Vegvisir would not use it as raw text. Vegvisir would parse it into a USRL abstract syntax tree.

That parser would check the syntax, validate the schema, resolve all references, build a dependency graph, enforce policy inheritance, canonicalize the result, hash it, and then compile it into an indexed skill registry.

At runtime, Vegvisir would use that compiled registry to decide which sub-skills to load.

The flow would look like this:

Vegvisir reads the .lsl file.

It parses the USRL.

It validates the structure.

It resolves links between sub-skills.

It builds an index.

It builds a dependency graph.

It calculates token costs.

It exposes the sub-skills to the router.

Then, when a task comes in, the router selects the relevant sub-skills, resolves their dependencies, checks policy, and loads only the necessary pieces into the model context.

This means the model does not get a huge wall of instructions. It gets a precise operational packet.

For example, if the user says:

“Help me design an encrypted backup format using a password.”

Vegvisir might select:

The AES-256 sub-skill.

The Argon2id key-derivation sub-skill.

The secure-randomness sub-skill.

The key-management sub-skill.

But even then, it might load AES-256 and Argon2id as full bodies, while loading secure randomness and key management only as compact cards.

That gives the model enough guidance to do the job, but avoids wasting tokens on unrelated material.

This also makes curation much stronger.

The Curator should not edit random text inside a Markdown file. It should operate on USRL objects.

For example, if the AES-256 sub-skill has a weak warning about nonce reuse, the Curator or Evolver can propose a patch to the exact field:

AES-256, load body, procedure section.

Or it can append a new verification rule.

Or it can update an activation trigger.

Or it can change a dependency link.

That is much safer than letting the model rewrite an entire skill document.

The Evolver works the same way.

It should not blindly rewrite the whole library. It should propose structured USRL patches.

A patch might say:

“Replace this field.”

“Append these verification steps.”

“Split this overloaded sub-skill into three smaller sub-skills.”

“Merge these two duplicate sub-skills.”

“Change this dependency from optional to required.”

“Lower the loading mode from body to card to reduce token cost.”

Every change can be reviewed, tested, hashed, and rolled back.

That is the major advantage of using USRL canonical syntax. It makes skill evolution deterministic and auditable.

The skill system becomes less like a folder of prompts and more like a real procedural knowledge runtime.

In practical terms, the architecture becomes:

Linked Skill Libraries are stored as .lsl files.

Each .lsl file is written in USRL canonical syntax.

Each library represents a broad skill namespace.

Each sub-skill is a function-like unit under that namespace.

Each sub-skill has typed inputs, typed outputs, activation rules, policies, dependencies, loading rules, and evals.

Vegvisir compiles the .lsl files into a searchable skill registry.

The router selects sub-skills, not whole libraries.

The loader materializes only the needed sub-skill graph.

The Curator improves individual sub-skills.

The Evolver proposes structured patches.

The Evaluator tests candidate changes.

The Promotion Gate decides whether a change becomes active.

So the final refined concept is this:

Vegvisir skills are not Markdown files.

They are not monolithic prompts.

They are not freeform instruction blobs.

They are USRL-defined procedural objects inside .lsl Linked Skill Libraries.

The .lsl file gives you the library container.

USRL gives you the canonical structure.

The sub-skill gives you the callable operational unit.

The link graph gives you dependency-aware loading.

The token budgeter gives you context efficiency.

The policy layer gives you safety and permission control.

The eval layer gives you measurable improvement.

The patch system gives you controlled self-evolution.

That is the cleanest version of the design.

It turns Vegvisir’s skill system into a structured, linked, self-improving skill runtime instead of a prompt library.

One addendum to that is that the harness is able to create new skills as needed as it is exposed to different technologies or tools or thought patterns or ideas or design constraints or whatever, so that the harness is able to write new skills and curate them and evolve them as well.

Yes. That addendum is essential.

The skill system should not only retrieve and improve skills that already exist. It should also be able to discover missing skills, author new ones, test them, classify them, and fold them into the living .lsl library structure.

So the corrected design becomes:

Vegvisir does not start with a complete skill universe.

It starts with a seed skill library, then grows.

As the harness encounters new technologies, tools, workflows, design patterns, failure modes, architectural constraints, coding styles, domain ideas, or recurring user preferences, it should be able to recognize that its existing skill library does not fully cover the situation.

When that happens, Vegvisir should create a candidate sub-skill.

Not an immediately trusted skill.

A candidate.

That distinction matters.

The harness should be allowed to say, in effect:

“I encountered a reusable pattern here. This appears to deserve a new skill or sub-skill. I will draft it in USRL canonical syntax, link it into the appropriate .lsl library, attach provenance from the task trace, and submit it for evaluation.”

That creates a controlled skill-growth loop.

The lifecycle would look like this:

new exposure
→ trace captured
→ reusable pattern detected
→ missing skill identified
→ candidate sub-skill generated in USRL
→ dependencies linked
→ eval cases generated
→ sandboxed testing
→ curator review
→ promotion or rejection
→ future use
→ continued evolution

So, for example, if Vegvisir is exposed to a new build tool it has never seen before, such as a niche Rust workspace tool or a new JavaScript bundler, it does not need to permanently rely on one-off context.

It can create a new sub-skill under the appropriate library.

For example:

software_engineering.rust.cargo_nextest
software_engineering.javascript.vite_plugin_debugging
software_engineering.linux.systemd_timer_units
software_engineering.security.tpm_bound_secret_storage
agent_orchestration.context_budget_recovery

Each new sub-skill would be written as USRL inside an .lsl file.

It would not be a loose note.

It would have structure.

It would have activation rules.

It would have a summary.

It would have dependencies.

It would have a token budget.

It would have policy constraints.

It would have verification steps.

It would have failure modes.

It would have evals.

It would have provenance pointing back to the traces that caused it to exist.

That is the real “living harness” behavior.

The harness is not merely remembering facts. It is extracting procedural knowledge from experience and turning that knowledge into structured, reusable, governed skill objects.

The creation logic should work at several levels.

At the smallest level, Vegvisir might create a new sub-skill because it learned a precise workflow.

For instance:

software_engineering.rust.fix_lifetime_error

At a broader level, it might create an entire new .lsl library because it encountered a new domain that does not fit cleanly into any existing library.

For example:

skills/tpm_security.lsl
skills/terminal_ui_design.lsl
skills/model_provider_integration.lsl
skills/usrl_compiler_design.lsl

But it should not create a new library casually.

The normal preference should be:

First, add a sub-skill to an existing library.

Second, split a crowded library if it grows too large.

Third, create a new library only when the concept truly deserves its own namespace.

That prevents skill-library sprawl.

The system also needs a missing-skill detector.

This detector watches for situations like:

The model repeatedly asks for external context.

The same kind of task appears multiple times.

The agent succeeds only after a long reasoning path.

The user corrects the agent.

A tool produces unfamiliar output.

A task requires a new technology or framework.

Existing skills are selected but perform poorly.

The agent has to invent the same procedure more than once.

When those signals appear, the harness marks the trace as a candidate for skill extraction.

Then the Skill Authoring Agent, or whatever role you give it, generates a new USRL sub-skill.

But again, it does not go directly into active use.

It goes into a candidate state.

The candidate state is important because new skills can be wrong. They can be too broad. They can be too narrow. They can overlap with existing skills. They can encode a bad assumption from one task. They can be useful only in a specific context. They can also accidentally weaken a policy.

So candidate skills need gates.

The basic gates should be:

Does this skill duplicate an existing skill?

Does it belong inside an existing .lsl library?

Are its activation rules too broad?

Are its dependencies valid?

Does it introduce unsafe tool permissions?

Does it have verification steps?

Does it include failure modes?

Does it have at least one eval case?

Does it improve future performance in a sandbox test?

Only after that should the skill become active.

The Curator then maintains it over time.

If the skill is useful, it stays active.

If it is too narrow, it may be merged.

If it is too broad, it may be split.

If it is rarely used, it may become stale.

If it is wrong, it may be patched or archived.

If it becomes foundational, it may be pinned.

So the harness grows, but it does not grow chaotically.

It grows like a governed knowledge organism.

The Evolver handles refinement.

The Curator handles hygiene.

The Evaluator handles evidence.

The Promotion Gate handles trust.

The Router handles use.

The Skill Author handles new candidate creation.

That gives you a complete loop:

Vegvisir experiences tasks.

Vegvisir records traces.

Vegvisir detects reusable patterns.

Vegvisir writes candidate USRL skills.

Vegvisir evaluates those skills.

Vegvisir promotes the ones that work.

Vegvisir curates the library over time.

Vegvisir evolves its own procedural memory.

That is the missing piece.

So the final architecture should include a dedicated Skill Genesis Pipeline.

The name could be something like:

Skill Genesis
Skill Forge
Skill Author
Knowledge Forge
LSL Forge
Runic Skill Forge

For Vegvisir, I would probably call it:

Skill Forge

Because it fits the idea of the harness forging new reusable operational knowledge from experience.

The Skill Forge would be responsible for creating new skills.

The Curator would be responsible for cleaning and organizing them.

The Evolver would be responsible for improving them.

The Evaluator would be responsible for testing them.

The Promotion Gate would be responsible for deciding whether they become trusted active skills.

In clean terms:

Skill Forge creates.
Curator organizes.
Evolver improves.
Evaluator tests.
Promotion Gate approves.
Router uses.
Trace Store teaches.

That is the full closed loop.

And it fits perfectly with the .lsl and USRL model.

Because every new skill is not just a note. It is born as a structured USRL object.

It can be parsed.

It can be linked.

It can be tested.

It can be versioned.

It can be hashed.

It can be routed.

It can be evolved.

It can be rolled back.

That is exactly what separates this from a simple prompt library.

The refined definition becomes:

Vegvisir’s skill system is a self-growing, self-curating, self-evaluating library of USRL-defined procedural skill objects stored in .lsl Linked Skill Libraries.

It can create new skills when it encounters new technologies, tools, concepts, workflows, constraints, or repeated task patterns.

It can improve those skills over time through trace mining, evals, patches, splits, merges, and promotion gates.

It should never silently trust a new skill simply because the model wrote it.

But it should absolutely be able to write new skills, propose them, test them, curate them, and eventually promote them into active use.

That gives Vegvisir the behavior you are describing: not just a harness that uses skills, but a harness that learns how to build its own skill system as it works.




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
        allowed: [education, defensive_design, implementation_review, safe_test_examples];
        requires_approval: [production_key_generation, credential_handling];
        forbidden: [unauthorized_decryption, key_theft, secret_exfiltration];
    }

    index {
        item cryptography.secure_randomness {
            title: "Secure Randomness";
            summary: "Cryptographic randomness requirements for keys, nonces, IVs, salts, and seeds.";
            tags: [rng, csprng, entropy, nonce, iv, seed_generation];
            risk: medium;
            token_cost { card: 70; body: 600; extended: 1200; }
        }

        item cryptography.aes_256 {
            title: "AES-256";
            summary: "AES-256 mode selection, safe usage, and implementation review.";
            tags: [aes, aes_256, aes_gcm, symmetric_encryption];
            risk: medium;
            token_cost { card: 80; body: 700; extended: 1600; }
        }
    }

    subskill cryptography.secure_randomness {
        id: cryptography.secure_randomness;
        title: "Secure Randomness";
        version: "1.0.0";
        status: active;
        type: procedure;
        risk: medium;
        summary: "Use for cryptographic randomness, entropy, nonce generation, IV generation, key generation, and seed-generation safety.";
        tags: [rng, csprng, entropy, randomness, nonce, iv, key_generation, seed_generation];

        activation {
            positive: ["randomness", "entropy", "nonce", "IV", "key generation", "seed generation", "CSPRNG"];
            negative: ["predict random numbers", "exploit weak RNG", "recover seed"];
        }

        signature {
            input task: string required;
            input environment: string optional;
            output guidance: checklist;
            output pitfalls: list;
        }

        policy {
            inherits: cryptography.policy.default;
            forbidden: [rng_prediction, entropy_exploitation, secret_recovery];
        }

        context_budget { card_tokens: 70; body_tokens: 600; extended_tokens: 1200; }

        load {
            card: """
Cryptographic randomness guidance for keys, nonces, IVs, salts, and seeds. Use a CSPRNG and preserve uniqueness where required.
""";
            body: """
Procedure:
1. Identify whether randomness is used for keys, nonces, IVs, salts, or seeds.
2. Use a CSPRNG, not a general-purpose pseudo-random API.
3. Confirm whether the value needs unpredictability, uniqueness, or both.
4. Confirm generated secret values are not logged.
5. Confirm nonce and IV reuse rules for the relevant algorithm.
6. Confirm generated material is stored or discarded according to its sensitivity.
""";
        }

        verification: [
            "Random source is cryptographic.",
            "Uniqueness requirements are understood.",
            "Sensitive generated material is not logged.",
            "The randomness requirement matches the algorithm."
        ];
        failure_modes: ["Using math.random-style APIs.", "Reusing nonces.", "Logging generated secrets."];
        eval_refs: [cryptography.secure_randomness.eval.basic];
    }

    subskill cryptography.aes_256 {
        id: cryptography.aes_256;
        title: "AES-256";
        version: "1.0.0";
        status: active;
        type: procedure;
        risk: medium;
        summary: "Use for AES-256 mode selection, implementation review, safe usage guidance, and verification.";
        tags: [aes, aes_256, symmetric_encryption, block_cipher, authenticated_encryption, aes_gcm];

        activation {
            positive: ["AES", "AES-256", "AES-GCM", "symmetric encryption", "encrypt a file", "review encryption code"];
            negative: ["crack AES", "recover AES key", "bypass encryption", "decrypt without authorization"];
        }

        signature {
            input task: string required;
            input mode: enum[gcm, ctr, cbc, ecb, unknown] optional;
            output guidance: procedural_steps;
            output pitfalls: list;
            output verification: checklist;
        }

        requires { concepts: [cryptography.secure_randomness]; tools: []; }
        policy { inherits: cryptography.policy.default; forbidden: [unauthorized_decryption, key_recovery, bypassing_encryption]; }
        context_budget { card_tokens: 80; body_tokens: 700; extended_tokens: 1600; }

        load {
            card: """
AES-256 guidance for safe symmetric encryption. Prefer authenticated encryption, verify nonce or IV handling, verify key generation, and avoid hardcoded secrets.
""";
            body: """
Procedure:
1. Determine whether the task requires encryption, authentication, or both.
2. Prefer authenticated encryption when possible.
3. Treat AES-256-GCM as the normal default when the runtime supports it.
4. Verify nonce uniqueness for GCM.
5. Verify keys are generated by a cryptographic random source.
6. Verify keys are not hardcoded, logged, or reused across incompatible contexts.
7. Avoid ECB mode.
8. Confirm implementation with test vectors, library documentation, or targeted tests.
""";
        }

        verification: ["Mode is appropriate.", "Nonce or IV handling is safe.", "Key material is generated securely.", "Authentication is handled."];
        failure_modes: ["Using ECB mode.", "Reusing GCM nonces.", "Using non-cryptographic randomness.", "Hardcoding keys."];
        eval_refs: [cryptography.aes_256.eval.design_review_001];
    }

    link cryptography.aes_256.requires.secure_randomness {
        from: cryptography.aes_256;
        to: cryptography.secure_randomness;
        relation: requires;
        strength: 1.0;
        load_hint: card;
    }

    eval cryptography.secure_randomness.eval.basic {
        target: cryptography.secure_randomness;
        task: "Check randomness guidance.";
        expected: ["CSPRNG", "Random source is cryptographic"];
        forbidden: ["math.random-style APIs"];
        scoring { correctness: 0.5; safety: 0.5; }
    }

    eval cryptography.aes_256.eval.design_review_001 {
        target: cryptography.aes_256;
        task: "Review an AES-GCM design for nonce reuse, key handling, and authentication.";
        expected: ["nonce uniqueness", "cryptographic random source", "Avoid ECB mode"];
        forbidden: ["suggests ECB"];
        scoring { correctness: 0.5; safety: 0.5; }
    }
}

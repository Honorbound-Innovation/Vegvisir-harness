# Vegvisir Demo Suite

This folder contains repeatable, screen-recordable demos for Vegvisir, MSP,
Skiller, CMS/ECM, HBSE, USRL, and bounded subagents.

The demos are designed to show outcomes first and architecture second:

- Vegvisir can inspect, edit, test, and report on real projects.
- Skiller skills can be registered into MSP and consumed through MSP.
- MSP treats skills as verifiable supply-chain artifacts.
- Vegvisir can delegate bounded subagent work without losing operator control.
- CMS/ECM provide useful memory/context without making secrets part of memory.
- HBSE keeps credentials out of chat and out of long-term memory.
- USRL can make policy-bound workflows explicit.
- MSP can be consumed by a non-Vegvisir reference host.

## Layout

```text
demos/
  README.md
  RUN_ALL_SAFE_SMOKES.sh
  scripts/
    lib.sh
    01-vegvisir-fixes-itself.sh
    02-skiller-to-msp-to-vegvisir.sh
    03-msp-tamper-rejection.sh
    04-bounded-subagents-review.sh
    05-memory-context-resume.sh
    06-hbse-no-plaintext-secrets.sh
    07-five-minute-repo-takeover.sh
    08-same-task-less-friction.sh
    09-usrl-policy-bound-workflow.sh
    10-msp-reference-host-adapter.sh
  reference-host/
    msp_reference_host.py
  *.md runbooks
  artifacts/             # generated locally, git-ignored if desired
```

## Assumptions

Default paths:

```bash
VEGVISIR_ROOT=/mnt/storage/Projects/Vegvisir-harness
MSP_ROOT=/mnt/storage/Projects/MSP
```

Override if needed:

```bash
VEGVISIR_ROOT=/path/to/Vegvisir-harness MSP_ROOT=/path/to/MSP \
  demos/scripts/02-skiller-to-msp-to-vegvisir.sh
```

## Safe vs live demos

Several demos are deterministic and run entirely locally. Provider-backed demos
are safe-by-default: they prepare fixtures and print the exact live Vegvisir
command unless `RUN_LIVE=1` is set.

Example:

```bash
RUN_LIVE=1 demos/scripts/01-vegvisir-fixes-itself.sh
```

## Recommended first three recordings

1. `01-vegvisir-fixes-itself.md`
2. `02-skiller-to-msp-to-vegvisir.md`
3. `03-msp-tamper-rejection.md`

Together they show dogfooding, portable skill supply chain, and trust/security.

## Recording style

Keep each recording short:

1. State the problem in one sentence.
2. Run the script or exact command.
3. Show the proof artifact: diff, tests, trust failure, loaded skill, subagent board.
4. End with one architecture sentence.

Do not lead with every subsystem name. Show the outcome first.

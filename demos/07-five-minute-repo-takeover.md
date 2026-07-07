# Demo 07 — Five-Minute Repo Takeover

## Goal

Drop Vegvisir into an unfamiliar repo and show it orienting, discovering tests,
running verification, making a low-risk improvement, and summarizing evidence.

## One-line pitch

> New repo to tested patch with minimal ceremony.

## Script

```bash
demos/scripts/07-five-minute-repo-takeover.sh
```

Safe mode creates a small repo fixture and prints the live command. To run live:

```bash
RUN_LIVE=1 demos/scripts/07-five-minute-repo-takeover.sh
```

Against another local repo:

```bash
RUN_LIVE=1 DEMO_WORKSPACE=/path/to/repo demos/scripts/07-five-minute-repo-takeover.sh
```

## What to show

1. Repo contents before Vegvisir starts.
2. Vegvisir identifies language/build system.
3. Vegvisir runs the appropriate test command.
4. Vegvisir makes one safe improvement if appropriate.
5. Vegvisir reruns verification and summarizes.

## What this proves

- Workspace orientation works.
- Tool use is low-friction.
- Build/test discovery is practical.
- Vegvisir can operate as a daily engineering workbench.

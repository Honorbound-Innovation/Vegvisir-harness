# Third-Party Notices

Vegvisir's original source code is licensed under the MIT License. See
[`LICENSE`](LICENSE) and [`licenses/MIT.txt`](licenses/MIT.txt).

Solarium and GhidraHeadlessMCP are first-party component systems owned by the
Vegvisir project owner and are covered by the Vegvisir MIT License in this
repository. They are listed here for clarity because they live under
`components/`, but they are not third-party vendored tools.

Ghidra itself is not vendored or redistributed by this repository. Vegvisir's
Ghidra integration expects a separately installed upstream Ghidra distribution
and discovers it through `GHIDRA_HOME`, `GHIDRA_HEADLESS`, or PATH. That external
installation retains its own upstream license, notices, and third-party license
texts outside the Vegvisir repository.

This file is a human-readable summary, not a substitute for license texts. When
distributing Vegvisir, include this file, `NOTICE`, `LICENSE`, and the
`licenses/` directory.

## License layout

```text
Vegvisir/
  LICENSE
  NOTICE
  THIRD_PARTY_NOTICES.md
  licenses/
    MIT.txt
    first-party/
      solarium/
      ghidra-headless-mcp/
```

## Component summary

| Component | Path | License / Notice summary |
| --- | --- | --- |
| Vegvisir original code | `vegvisir/`, root scripts/docs authored for Vegvisir | MIT |
| Solarium | `components/solarium/` | First-party Vegvisir component; covered by Vegvisir MIT License |
| GhidraHeadlessMCP | `components/ghidra-headless-mcp/` | First-party Vegvisir component; covered by Vegvisir MIT License |
| Ghidra | external installed runtime | Not redistributed; see the installed Ghidra distribution for its Apache-2.0 license, NOTICE, GPL support-material notices, and bundled third-party license texts |

## First-party components

The following component systems are owned by the Vegvisir project owner and are
included as first-party Vegvisir code under the repository MIT License:

```text
components/solarium/
components/ghidra-headless-mcp/
```

Mirrored MIT license pointers are provided at:

```text
licenses/first-party/solarium/MIT.txt
licenses/first-party/ghidra-headless-mcp/MIT.txt
```

## Ghidra runtime integration

Install Ghidra separately from upstream releases or your system package manager,
then expose it to Vegvisir with one of:

```bash
export GHIDRA_HOME=/path/to/ghidra_<version>
export GHIDRA_HEADLESS="$GHIDRA_HOME/support/analyzeHeadless"
# or place analyzeHeadless / ghidraRun on PATH
```

The Vegvisir repository does not contain `components/ghidra/`, mirrored Ghidra
source notices, Ghidra GPL support materials, or Ghidra third-party license
bundles. Those files belong to the external Ghidra installation.

## Redistribution note

If Vegvisir is packaged or redistributed, include:

- `LICENSE`
- `NOTICE`
- `THIRD_PARTY_NOTICES.md`
- `licenses/`
- original license/notice files for any vendored third-party component added in the future

If new components are vendored or first-party component ownership changes,
update this file and `NOTICE` before release.

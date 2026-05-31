# Third-Party Notices

Vegvisir's original source code is licensed under the MIT License. See
[`LICENSE`](LICENSE) and [`licenses/MIT.txt`](licenses/MIT.txt).

Solarium and GhidraHeadlessMCP are first-party component systems owned by the
Vegvisir project owner and are covered by the Vegvisir MIT License in this
repository. They are listed here for clarity because they live under
`components/`, but they are not third-party vendored tools.

Third-party component systems under `components/` keep their own licenses and
notices. The root MIT license applies to Vegvisir-authored code and first-party
components; it does **not** erase, replace, or relicense third-party component
terms.

This file is a human-readable summary, not a substitute for the license texts.
When distributing Vegvisir, include this file, `NOTICE`, `LICENSE`, and the
`licenses/` directory.

## License layout

```text
Vegvisir/
  LICENSE
  NOTICE
  THIRD_PARTY_NOTICES.md
  licenses/
    MIT.txt
    Apache-2.0.txt
    Ghidra-NOTICE.txt
    first-party/
      solarium/
      ghidra-headless-mcp/
    third-party/
      ghidra/
      ghidra-gpl/
      ghidra-mcp/
```

## Component summary

| Component | Path | License / Notice summary |
| --- | --- | --- |
| Vegvisir original code | `vegvisir/`, root scripts/docs authored for Vegvisir | MIT |
| Solarium | `components/solarium/` | First-party Vegvisir component; covered by Vegvisir MIT License |
| GhidraHeadlessMCP | `components/ghidra-headless-mcp/` | First-party Vegvisir component; covered by Vegvisir MIT License |
| Ghidra | `components/ghidra/` | Apache License 2.0, plus upstream `NOTICE` and third-party license files |
| Ghidra GPL support programs | `components/ghidra/GPL/` | GPL/LGPL-family support materials as provided by upstream Ghidra |
| GhidraMCP | `components/ghidra-mcp/` | Apache License 2.0 |

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

## Ghidra

The vendored Ghidra source is under:

```text
components/ghidra/
```

Its primary license is Apache License 2.0:

```text
components/ghidra/LICENSE
licenses/Apache-2.0.txt
licenses/third-party/ghidra/LICENSE
```

Its upstream notice is preserved at:

```text
components/ghidra/NOTICE
licenses/Ghidra-NOTICE.txt
licenses/third-party/ghidra/NOTICE
```

Ghidra also ships third-party license texts under:

```text
components/ghidra/licenses/
licenses/third-party/ghidra/licenses/
```

The upstream Ghidra notice states that portions were developed at the National
Security Agency and that portions created by U.S. Government employees may not
be subject to U.S. copyright protections under 17 U.S.C.

## Ghidra GPL support materials

The vendored Ghidra tree includes:

```text
components/ghidra/GPL/
```

Ghidra's upstream `NOTICE` describes this directory as containing stand-alone
support programs released under GPL 3 and related licenses. The license texts
from that directory are mirrored at:

```text
licenses/third-party/ghidra-gpl/licenses/
```

Do not assume the root MIT license applies to these files.

## GhidraMCP

The vendored GhidraMCP source is under:

```text
components/ghidra-mcp/
```

It is licensed under Apache License 2.0:

```text
components/ghidra-mcp/LICENSE
licenses/third-party/ghidra-mcp/LICENSE
```

## Redistribution note

If Vegvisir is packaged or redistributed, include:

- `LICENSE`
- `NOTICE`
- `THIRD_PARTY_NOTICES.md`
- `licenses/`
- the original license/notice files preserved in vendored component directories

If new components are vendored or first-party component ownership changes,
update this file and `NOTICE` before release.

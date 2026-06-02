# Firmware Analysis

## Command

```bash
python3 -m biw.cli firmware triage ./firmware.img --out .analysis/cases
```

## MVP behavior

- hashes the image
- detects coarse file format
- extracts ASCII strings
- scans for embedded file magics such as ELF, PE, zip, gzip, squashfs, and scripts
- inventories zip/tar entries when applicable
- applies BIW heuristic findings
- writes a firmware report

The MVP does not execute firmware and does not destructively unpack images.

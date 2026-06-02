# Solarium Workbench UI

BIW includes a dependency-free static UI scaffold at `solarium/binary-workbench/`.

## Run

Terminal 1:

```bash
python3 -m biw.cli serve --out .analysis/cases --host 127.0.0.1 --port 8765
```

Terminal 2:

```bash
python3 -m http.server 8787 --directory solarium/binary-workbench
```

Open `http://127.0.0.1:8787`.

## Panels

The UI MVP provides:

- API health indicator
- case list from `/api/index`
- case summary view
- findings JSON view
- skill output view
- function report view
- artifact-count/artifact metadata view
- notes/report text view

## Integration contract

The UI intentionally consumes the same read-only API intended for Solarium integration. A future native Solarium panel can reuse `src/app.js` fetch behavior or replace it with Solarium-native state/components.

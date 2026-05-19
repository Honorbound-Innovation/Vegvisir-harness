import test from "node:test";
import assert from "node:assert/strict";

import { parseCll, parsePll, validatePair } from "../src/index.js";

test("validates a minimal paired pll/cll", () => {
  const pll = parsePll(`
    version "2.0";
    library PromptLibrary ExamplePrompts {
      metadata {
        Id: "pair-1";
        Name: "Example Prompt Library";
        Version: "1.0.0";
        Created: "2026-04-10T00:00:00Z";
        Modified: "2026-04-10T00:00:00Z";
        Author: "usrl";
        Source: "spec.md";
        Purpose: "demo";
      }

      prompt BuildApi {
        Id: "P.BuildApi";
        Title: "Build API";
        Purpose: "Generate API files";
        Type: "Implementation";
        Inputs: [];
        Outputs: [];
        Body: "Do work";
        Links: [];
      }
    }
  `);

  const cll = parseCll(`
    version "2.0";
    library ContractLibrary ExampleContracts {
      metadata {
        Id: "pair-1";
        Name: "Example Contract Library";
        Version: "1.0.0";
        Created: "2026-04-10T00:00:00Z";
        Modified: "2026-04-10T00:00:00Z";
        Author: "usrl";
        Source: "spec.md";
        Purpose: "governance";
      }

      contract_unit EnforceBuildApi {
        Id: "C.EnforceBuildApi";
        Title: "Enforce build api";
        Purpose: "must govern prompt";
        Scope: "Prompt";
        Rules: [{ Id: "R1"; Category: "Required"; Condition: "true"; Action: "accept"; Severity: "Error"; Target: "P.BuildApi"; }];
        Targets: ["P.BuildApi"];
        Validation: [{ Type: "schema"; Blocking: true; }];
        Links: [
          { Type: "Governs"; Target: "P.BuildApi"; Blocking: true; }
        ];
      }
    }
  `);

  const issues = validatePair(pll, cll);
  assert.equal(issues.length, 0);
});

test("flags unresolved blocking targets", () => {
  const pll = parsePll(`
    version "2.0";
    library PromptLibrary ExamplePrompts {
      metadata {
        Id: "pair-1";
        Name: "Example Prompt Library";
        Version: "1.0.0";
        Created: "2026-04-10T00:00:00Z";
        Modified: "2026-04-10T00:00:00Z";
        Author: "usrl";
        Source: "spec.md";
        Purpose: "demo";
      }

      prompt BuildApi {
        Id: "P.BuildApi";
        Title: "Build API";
        Purpose: "Generate API files";
        Type: "Implementation";
        Inputs: [];
        Outputs: [];
        Body: "Do work";
        Links: [];
      }
    }
  `);

  const cll = parseCll(`
    version "2.0";
    library ContractLibrary ExampleContracts {
      metadata {
        Id: "pair-1";
        Name: "Example Contract Library";
        Version: "1.0.0";
        Created: "2026-04-10T00:00:00Z";
        Modified: "2026-04-10T00:00:00Z";
        Author: "usrl";
        Source: "spec.md";
        Purpose: "governance";
      }

      contract_unit EnforceBuildApi {
        Id: "C.EnforceBuildApi";
        Title: "Enforce build api";
        Purpose: "must govern prompt";
        Scope: "Prompt";
        Rules: [{ Id: "R1"; Category: "Required"; Condition: "true"; Action: "accept"; Severity: "Error"; Target: "P.Missing"; }];
        Targets: ["P.Missing"];
        Validation: [{ Type: "schema"; Blocking: true; }];
        Links: [
          { Type: "Governs"; Target: "P.Missing"; Blocking: true; }
        ];
      }
    }
  `);

  const issues = validatePair(pll, cll);
  assert.ok(issues.some((i) => i.message.includes("unresolved target 'P.Missing'")));
});

test("flags strict schema field issues", () => {
  const pll = parsePll(`
    version "2.0";
    library PromptLibrary BadPrompts {
      metadata {
        Id: "pair-1";
        Name: "Bad Prompt Library";
        Version: "1.0.0";
        Created: "2026-04-10T00:00:00Z";
        Modified: "2026-04-10T00:00:00Z";
        Author: "usrl";
        Source: "spec.md";
        Purpose: "demo";
      }

      prompt Broken {
        Id: "P.Broken";
        Purpose: "bad";
        Type: "Implementation";
        Inputs: [];
        Outputs: [];
        Body: "Do work";
        Links: [];
      }
    }
  `);

  const cll = parseCll(`
    version "2.0";
    library ContractLibrary BadContracts {
      metadata {
        Id: "pair-1";
        Name: "Bad Contract Library";
        Version: "1.0.0";
        Created: "2026-04-10T00:00:00Z";
        Modified: "2026-04-10T00:00:00Z";
        Author: "usrl";
        Source: "spec.md";
        Purpose: "governance";
      }

      contract_unit Broken {
        Id: "C.Broken";
        Title: "broken";
        Purpose: "broken";
        Scope: "Prompt";
        Rules: [];
        Targets: [];
        Validation: [];
        Links: [];
      }
    }
  `);

  const issues = validatePair(pll, cll);
  assert.ok(issues.some((i) => i.message.includes("missing required field 'Title'")));
  assert.ok(issues.some((i) => i.message.includes("field 'Rules' must be non-empty")));
  assert.ok(issues.some((i) => i.message.includes("field 'Validation' must be non-empty")));
});

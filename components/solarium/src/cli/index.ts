#!/usr/bin/env node
import { readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { mkdir } from "node:fs/promises";
import { Command } from "commander";
import { browse } from "../agent/actions.js";
import { runActions } from "../agent/session.js";
import { inspectPage } from "../agent/inspect.js";
import { planActionsFromInspectResult } from "../agent/plan.js";
import { runLoop } from "../agent/loop.js";
import { readSolariumJob, runJob } from "../config/job.js";
import { getBuiltInProfile, listBuiltInProfiles, readBrowserProfile, summarizeProfile, validateBrowserProfile } from "../browser/profile-store.js";
import { createAuthSessionProfile, readAuthSessionProfile, resolveAuthSession } from "../browser/auth-session.js";
import { validateSolariumFile, type SolariumValidationKind } from "../config/validate.js";
import { audit } from "../security/audit.js";
import { graphqlAudit } from "../security/graphql-audit.js";
import { owaspAudit, type OwaspAuditProfile } from "../security/owasp-audit.js";
import { crawl } from "../security/crawler.js";
import { createJsonlEventLogger } from "../reporting/events.js";
import { createSessionResumePlan, replayEvents } from "../reporting/replay.js";
import { renderAuditHtmlReport, renderCrawlHtmlReport, renderGraphqlAuditHtmlReport, renderOwaspAuditHtmlReport, renderLoopHtmlReport, renderSessionHtmlReport } from "../reporting/html.js";
import { renderAuditMarkdownReport, renderCrawlMarkdownReport, renderGraphqlAuditMarkdownReport, renderOwaspAuditMarkdownReport, renderLoopMarkdownReport, renderSessionMarkdownReport } from "../reporting/markdown.js";
import { createArtifactManifest } from "../reporting/artifacts.js";
import { createEvidenceRunManifest, type EvidenceRunKind } from "../reporting/evidence.js";
import { createWorkflowSeedFromFiles } from "../skills/workflow-seed.js";
import { runJsonRpcServer } from "../server/json-rpc.js";
import { checkUrlScope, validateScopePolicy, type ScopePolicy } from "../security/scope.js";
import type { AgentAction, BrowserEngine, BrowserProfile, BrowserProfileName, InspectResult } from "../types.js";

const program = new Command();

program
  .name("solarium")
  .description("Agent-controlled browser runtime for automation, research, and authorized security testing")
  .version("0.1.0");

program
  .command("browse")
  .description("Open a URL in a controlled browser context")
  .argument("<url>", "URL to browse")
  .option("-e, --engine <engine>", "Browser engine: chromium, firefox, or webkit", "chromium")
  .option("-p, --profile <profile>", "Browser profile", "chrome-stable")
  .option("--profile-file <path>", "Path to a custom browser profile JSON file")
  .option("--storage-state <path>", "Load Playwright browser context storage state from a JSON file")
  .option("--save-storage-state <path>", "Save browser context storage state to a JSON file before closing")
  .option("--auth-session <path>", "Path to a Solarium auth-session profile JSON file")
  .option("--downloads-dir <path>", "Directory where browser downloads should be accepted and stored")
  .option("--scope <path>", "Path to a JSON scope policy file")
  .option("--headed", "Run with a visible browser window")
  .option("--screenshot <path>", "Save a full-page screenshot")
  .option("--extract-text", "Extract visible text from the page")
  .option("--observe", "Return a structured page observation for agent use")
  .option("--observation <path>", "Write structured page observation JSON to a file")
  .option("--max-text-chars <number>", "Maximum observed visible-text characters", parseInteger)
  .option("--max-elements <number>", "Maximum links/buttons/inputs/forms to observe", parseInteger)
  .option("--trace", "Record a Playwright trace")
  .action(async (url: string, options: Record<string, unknown>) => {
    try {
      const scope = await readOptionalScopePolicy(options.scope as string | undefined);
      const result = await browse({
        url,
        engine: options.engine as BrowserEngine,
        profile: await resolveCliProfile(options),
        headless: !options.headed,
        storageState: (await resolveCliAuthSession(options)).storageState,
        saveStorageState: (await resolveCliAuthSession(options)).saveStorageState,
        downloadsDir: options.downloadsDir as string | undefined,
        screenshotPath: options.screenshot as string | undefined,
        extractText: Boolean(options.extractText),
        observe: Boolean(options.observe),
        observationPath: options.observation as string | undefined,
        observationOptions: {
          maxTextChars: options.maxTextChars as number | undefined,
          maxElements: options.maxElements as number | undefined
        },
        scope,
        trace: Boolean(options.trace)
      });

      console.log(JSON.stringify(result, null, 2));
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });

program
  .command("session")
  .description("Run a multi-step agent browser session from a JSON action list")
  .requiredOption("-a, --actions <path>", "Path to a JSON file containing AgentAction[]")
  .option("-o, --output <path>", "Write the session result JSON to a file")
  .option("--report <path>", "Write a Markdown session report to a file")
  .option("--html-report <path>", "Write an HTML session report to a file")
  .option("--report-include-json", "Include the full JSON result as a report appendix")
  .option("--events <path>", "Write a JSONL session event timeline")
  .option("--resume-from <path>", "Resume by skipping steps already marked successful in a prior JSONL session event log")
  .option("-e, --engine <engine>", "Browser engine: chromium, firefox, or webkit", "chromium")
  .option("-p, --profile <profile>", "Browser profile", "chrome-stable")
  .option("--profile-file <path>", "Path to a custom browser profile JSON file")
  .option("--storage-state <path>", "Load Playwright browser context storage state from a JSON file")
  .option("--save-storage-state <path>", "Save browser context storage state to a JSON file before closing")
  .option("--auth-session <path>", "Path to a Solarium auth-session profile JSON file")
  .option("--downloads-dir <path>", "Directory where browser downloads should be accepted and stored")
  .option("--scope <path>", "Path to a JSON scope policy file")
  .option("--headed", "Run with a visible browser window")
  .option("--session-id <id>", "Stable session identifier")
  .option("--evidence-dir <path>", "Directory for generated session evidence")
  .option("--no-observe-after-each-action", "Disable automatic observations after every action")
  .option("--continue-on-error", "Continue executing later actions after a failed step")
  .option("--max-text-chars <number>", "Maximum observed visible-text characters", parseInteger)
  .option("--max-elements <number>", "Maximum links/buttons/inputs/forms to observe", parseInteger)
  .option("--trace", "Record a Playwright trace")
  .action(async (options: Record<string, unknown>) => {
    try {
      const originalActions = await readActionsFile(options.actions as string);
      const resumePlan = options.resumeFrom
        ? await createSessionResumePlan(originalActions, options.resumeFrom as string)
        : undefined;
      const actions = resumePlan?.remainingActions ?? originalActions;
      const scope = await readOptionalScopePolicy(options.scope as string | undefined);
      const result = await runActions({
        actions,
        actionOffset: resumePlan?.resumeFromStep ?? 0,
        engine: options.engine as BrowserEngine,
        profile: await resolveCliProfile(options),
        headless: !options.headed,
        storageState: (await resolveCliAuthSession(options)).storageState,
        saveStorageState: (await resolveCliAuthSession(options)).saveStorageState,
        downloadsDir: options.downloadsDir as string | undefined,
        sessionId: options.sessionId as string | undefined,
        evidenceDir: options.evidenceDir as string | undefined,
        observeAfterEachAction: options.observeAfterEachAction as boolean | undefined,
        continueOnError: Boolean(options.continueOnError),
        observationOptions: {
          maxTextChars: options.maxTextChars as number | undefined,
          maxElements: options.maxElements as number | undefined
        },
        scope,
        trace: Boolean(options.trace),
        eventLogger: createJsonlEventLogger(options.events as string | undefined)
      });

      const output = resumePlan ? { ...result, resume: resumePlan } : result;
      const json = JSON.stringify(output, null, 2);
      if (options.output) {
        await writeTextFile(options.output as string, json);
      }
      if (options.report) {
        await writeTextFile(
          options.report as string,
          renderSessionMarkdownReport(result, { includeJsonAppendix: Boolean(options.reportIncludeJson) })
        );
      }
      if (options.htmlReport) {
        await writeTextFile(
          options.htmlReport as string,
          renderSessionHtmlReport(result, { includeJsonAppendix: Boolean(options.reportIncludeJson) })
        );
      }
      console.log(json);
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });

program
  .command("crawl")
  .description("Crawl in-scope pages and inventory links/forms for authorized research")
  .argument("<url>", "Starting URL")
  .requiredOption("--scope <path>", "Path to a JSON scope policy file with allowedHosts")
  .option("-o, --output <path>", "Write the crawl result JSON to a file")
  .option("--report <path>", "Write a Markdown crawl report to a file")
  .option("--html-report <path>", "Write an HTML crawl report to a file")
  .option("--report-include-json", "Include the full JSON result as a report appendix")
  .option("--events <path>", "Write a JSONL crawl event timeline")
  .option("-e, --engine <engine>", "Browser engine: chromium, firefox, or webkit", "chromium")
  .option("-p, --profile <profile>", "Browser profile", "chrome-stable")
  .option("--profile-file <path>", "Path to a custom browser profile JSON file")
  .option("--storage-state <path>", "Load Playwright browser context storage state from a JSON file")
  .option("--save-storage-state <path>", "Save browser context storage state to a JSON file before closing")
  .option("--auth-session <path>", "Path to a Solarium auth-session profile JSON file")
  .option("--downloads-dir <path>", "Directory where browser downloads should be accepted and stored")
  .option("--headed", "Run with a visible browser window")
  .option("--max-pages <number>", "Maximum pages to visit", parseInteger, 10)
  .option("--max-depth <number>", "Maximum link depth from start URL", parseInteger, 1)
  .option("--evidence-dir <path>", "Directory for observations and optional screenshots")
  .option("--screenshots", "Capture a screenshot for each visited page when evidence-dir is set")
  .option("--include-observations", "Embed full observations in the crawl result JSON")
  .option("--wait-after-navigation-ms <number>", "Delay after each navigation before observing", parseInteger)
  .option("--max-text-chars <number>", "Maximum observed visible-text characters", parseInteger)
  .option("--max-elements <number>", "Maximum links/buttons/inputs/forms to observe", parseInteger)
  .option("--trace", "Record a Playwright trace")
  .action(async (url: string, options: Record<string, unknown>) => {
    try {
      const scope = await readScopePolicy(options.scope as string);
      const result = await crawl({
        startUrl: url,
        scope,
        engine: options.engine as BrowserEngine,
        profile: await resolveCliProfile(options),
        headless: !options.headed,
        storageState: (await resolveCliAuthSession(options)).storageState,
        saveStorageState: (await resolveCliAuthSession(options)).saveStorageState,
        downloadsDir: options.downloadsDir as string | undefined,
        maxPages: options.maxPages as number | undefined,
        maxDepth: options.maxDepth as number | undefined,
        evidenceDir: options.evidenceDir as string | undefined,
        screenshots: Boolean(options.screenshots),
        includeObservations: Boolean(options.includeObservations),
        waitAfterNavigationMs: options.waitAfterNavigationMs as number | undefined,
        observationOptions: {
          maxTextChars: options.maxTextChars as number | undefined,
          maxElements: options.maxElements as number | undefined
        },
        trace: Boolean(options.trace),
        eventLogger: createJsonlEventLogger(options.events as string | undefined)
      });

      const json = JSON.stringify(result, null, 2);
      if (options.output) {
        await writeTextFile(options.output as string, json);
      }
      if (options.report) {
        await writeTextFile(
          options.report as string,
          renderCrawlMarkdownReport(result, { includeJsonAppendix: Boolean(options.reportIncludeJson) })
        );
      }
      if (options.htmlReport) {
        await writeTextFile(
          options.htmlReport as string,
          renderCrawlHtmlReport(result, { includeJsonAppendix: Boolean(options.reportIncludeJson) })
        );
      }
      console.log(json);
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });

program
  .command("audit")
  .description("Passively audit an in-scope page for common defensive web security findings")
  .argument("<url>", "URL to audit")
  .option("--scope <path>", "Path to a JSON scope policy file")
  .option("-o, --output <path>", "Write the audit result JSON to a file")
  .option("--report <path>", "Write a Markdown audit report to a file")
  .option("--html-report <path>", "Write an HTML audit report to a file")
  .option("--report-include-json", "Include the full JSON result as a report appendix")
  .option("-e, --engine <engine>", "Browser engine: chromium, firefox, or webkit", "chromium")
  .option("-p, --profile <profile>", "Browser profile", "chrome-stable")
  .option("--profile-file <path>", "Path to a custom browser profile JSON file")
  .option("--storage-state <path>", "Load Playwright browser context storage state from a JSON file")
  .option("--save-storage-state <path>", "Save browser context storage state to a JSON file before closing")
  .option("--auth-session <path>", "Path to a Solarium auth-session profile JSON file")
  .option("--downloads-dir <path>", "Directory where browser downloads should be accepted and stored")
  .option("--headed", "Run with a visible browser window")
  .option("--include-observation", "Embed full page observation evidence in the audit result")
  .option("--wait-after-navigation-ms <number>", "Delay after navigation before observing", parseInteger)
  .option("--max-text-chars <number>", "Maximum observed visible-text characters", parseInteger)
  .option("--max-elements <number>", "Maximum links/buttons/inputs/forms to observe", parseInteger)
  .option("--trace", "Record a Playwright trace")
  .action(async (url: string, options: Record<string, unknown>) => {
    try {
      const scope = await readOptionalScopePolicy(options.scope as string | undefined);
      const result = await audit({
        url,
        scope,
        outputPath: options.output as string | undefined,
        engine: options.engine as BrowserEngine,
        profile: await resolveCliProfile(options),
        headless: !options.headed,
        storageState: (await resolveCliAuthSession(options)).storageState,
        saveStorageState: (await resolveCliAuthSession(options)).saveStorageState,
        downloadsDir: options.downloadsDir as string | undefined,
        includeObservation: Boolean(options.includeObservation),
        waitAfterNavigationMs: options.waitAfterNavigationMs as number | undefined,
        observationOptions: {
          maxTextChars: options.maxTextChars as number | undefined,
          maxElements: options.maxElements as number | undefined
        },
        trace: Boolean(options.trace)
      });

      if (options.report) {
        await writeTextFile(
          options.report as string,
          renderAuditMarkdownReport(result, { includeJsonAppendix: Boolean(options.reportIncludeJson) })
        );
      }
      if (options.htmlReport) {
        await writeTextFile(
          options.htmlReport as string,
          renderAuditHtmlReport(result, { includeJsonAppendix: Boolean(options.reportIncludeJson) })
        );
      }

      console.log(JSON.stringify(result, null, 2));
      if (!result.ok) {
        process.exitCode = 1;
      }
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });



program
  .command("owasp-audit")
  .description("Run a passive OWASP-mapped browser audit for an authorized page")
  .argument("<url>", "URL to audit")
  .option("--scope <path>", "Path to a JSON scope policy file")
  .option("--profile <profile>", "OWASP audit profile: passive, strict-headers, active-authorized, top10-passive, or top10-active-authorized", "passive")
  .option("-o, --output <path>", "Write the OWASP audit result JSON to a file")
  .option("--report <path>", "Write a Markdown OWASP audit report to a file")
  .option("--html-report <path>", "Write an HTML OWASP audit report to a file")
  .option("--report-include-json", "Include the full JSON result as a report appendix")
  .option("-e, --engine <engine>", "Browser engine: chromium, firefox, or webkit", "chromium")
  .option("--browser-profile <profile>", "Browser profile", "chrome-stable")
  .option("--profile-file <path>", "Path to a custom browser profile JSON file")
  .option("--storage-state <path>", "Load Playwright browser context storage state from a JSON file")
  .option("--save-storage-state <path>", "Save browser context storage state to a JSON file before closing")
  .option("--auth-session <path>", "Path to a Solarium auth-session profile JSON file")
  .option("--downloads-dir <path>", "Directory where browser downloads should be accepted and stored")
  .option("--headed", "Run with a visible browser window")
  .option("--wait-after-navigation-ms <number>", "Delay after navigation before observing", parseInteger)
  .option("--max-active-requests <number>", "Maximum additional active-authorized probes; hard-capped at 25", parseInteger)
  .option("--active-delay-ms <number>", "Delay between active-authorized probes", parseInteger)
  .option("--active-request-timeout-ms <number>", "Timeout for each active-authorized probe", parseInteger)
  .option("--max-text-chars <number>", "Maximum observed visible-text characters", parseInteger)
  .option("--max-elements <number>", "Maximum links/buttons/inputs/forms to observe", parseInteger)
  .option("--trace", "Record a Playwright trace")
  .action(async (url: string, options: Record<string, unknown>) => {
    try {
      const scope = await readOptionalScopePolicy(options.scope as string | undefined);
      const result = await owaspAudit({
        url,
        scope,
        owaspProfile: parseOwaspProfile(options.profile as string | undefined),
        outputPath: options.output as string | undefined,
        engine: options.engine as BrowserEngine,
        profile: await resolveCliProfile({ ...options, profile: options.browserProfile ?? "chrome-stable" }),
        headless: !options.headed,
        storageState: (await resolveCliAuthSession(options)).storageState,
        saveStorageState: (await resolveCliAuthSession(options)).saveStorageState,
        downloadsDir: options.downloadsDir as string | undefined,
        waitAfterNavigationMs: options.waitAfterNavigationMs as number | undefined,
        maxActiveRequests: options.maxActiveRequests as number | undefined,
        activeDelayMs: options.activeDelayMs as number | undefined,
        activeRequestTimeoutMs: options.activeRequestTimeoutMs as number | undefined,
        observationOptions: {
          maxTextChars: options.maxTextChars as number | undefined,
          maxElements: options.maxElements as number | undefined
        },
        trace: Boolean(options.trace)
      });

      if (options.report) {
        await writeTextFile(
          options.report as string,
          renderOwaspAuditMarkdownReport(result, { includeJsonAppendix: Boolean(options.reportIncludeJson) })
        );
      }
      if (options.htmlReport) {
        await writeTextFile(
          options.htmlReport as string,
          renderOwaspAuditHtmlReport(result, { includeJsonAppendix: Boolean(options.reportIncludeJson) })
        );
      }

      console.log(JSON.stringify(result, null, 2));
      if (!result.ok) process.exitCode = 1;
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });


program
  .command("graphql-audit")
  .alias("gql-audit")
  .description("Run bounded non-DoS GraphQL endpoint and schema security checks")
  .argument("<url>", "Base URL or GraphQL endpoint URL")
  .requiredOption("--scope <path>", "Path to a JSON scope policy file with allowedHosts")
  .option("--endpoint <url>", "Explicit GraphQL endpoint path or URL")
  .option("-o, --output <path>", "Write the GraphQL audit result JSON to a file")
  .option("--report <path>", "Write a Markdown GraphQL audit report to a file")
  .option("--html-report <path>", "Write an HTML GraphQL audit report to a file")
  .option("--report-include-json", "Include the full JSON result as a report appendix")
  .option("--timeout-ms <number>", "Per-request timeout in milliseconds", parseInteger)
  .option("--max-endpoints <number>", "Maximum endpoint candidates to probe", parseInteger)
  .option("--include-introspection-schema", "Include the full introspection response in JSON output")
  .option("--no-batch-check", "Skip the tiny two-operation batching check")
  .option("--safe-data-probes", "Run known read-only exposure probes when matching schema fields exist")
  .action(async (url: string, options: Record<string, unknown>) => {
    try {
      const scope = await readScopePolicy(options.scope as string);
      const result = await graphqlAudit({
        url,
        scope,
        endpoint: options.endpoint as string | undefined,
        outputPath: options.output as string | undefined,
        timeoutMs: options.timeoutMs as number | undefined,
        maxEndpoints: options.maxEndpoints as number | undefined,
        includeIntrospectionSchema: Boolean(options.includeIntrospectionSchema),
        batchCheck: options.batchCheck as boolean | undefined,
        safeDataProbes: Boolean(options.safeDataProbes)
      });

      if (options.report) {
        await writeTextFile(
          options.report as string,
          renderGraphqlAuditMarkdownReport(result, { includeJsonAppendix: Boolean(options.reportIncludeJson) })
        );
      }
      if (options.htmlReport) {
        await writeTextFile(
          options.htmlReport as string,
          renderGraphqlAuditHtmlReport(result, { includeJsonAppendix: Boolean(options.reportIncludeJson) })
        );
      }

      console.log(JSON.stringify(result, null, 2));
      if (!result.ok) process.exitCode = 1;
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });

program
  .command("inspect")
  .description("Inspect a page and return agent-actionable selector candidates")
  .argument("<url>", "URL to inspect")
  .option("--scope <path>", "Path to a JSON scope policy file")
  .option("-o, --output <path>", "Write the inspect result JSON to a file")
  .option("-e, --engine <engine>", "Browser engine: chromium, firefox, or webkit", "chromium")
  .option("-p, --profile <profile>", "Browser profile", "chrome-stable")
  .option("--profile-file <path>", "Path to a custom browser profile JSON file")
  .option("--storage-state <path>", "Load Playwright browser context storage state from a JSON file")
  .option("--save-storage-state <path>", "Save browser context storage state to a JSON file before closing")
  .option("--auth-session <path>", "Path to a Solarium auth-session profile JSON file")
  .option("--downloads-dir <path>", "Directory where browser downloads should be accepted and stored")
  .option("--headed", "Run with a visible browser window")
  .option("--screenshot <path>", "Save a full-page screenshot")
  .option("--include-observation", "Embed full page observation evidence in the inspect result")
  .option("--max-candidates <number>", "Maximum action candidates to return", parseInteger)
  .option("--max-text-chars <number>", "Maximum observed visible-text characters", parseInteger)
  .option("--max-elements <number>", "Maximum links/buttons/inputs/forms to observe", parseInteger)
  .option("--wait-after-navigation-ms <number>", "Delay after navigation before inspecting", parseInteger)
  .option("--actions-out <path>", "Write suggested session actions derived from inspect candidates")
  .option("--goal <text>", "Optional planning goal used to rank inspect candidates")
  .option("--plan-max-actions <number>", "Maximum actions to include in generated actions-out", parseInteger)
  .option("--fill-value <text>", "Value to use for generated fill/type actions")
  .option("--trace", "Record a Playwright trace")
  .action(async (url: string, options: Record<string, unknown>) => {
    try {
      const scope = await readOptionalScopePolicy(options.scope as string | undefined);
      const result = await inspectPage({
        url,
        scope,
        engine: options.engine as BrowserEngine,
        profile: await resolveCliProfile(options),
        headless: !options.headed,
        storageState: (await resolveCliAuthSession(options)).storageState,
        saveStorageState: (await resolveCliAuthSession(options)).saveStorageState,
        downloadsDir: options.downloadsDir as string | undefined,
        screenshotPath: options.screenshot as string | undefined,
        includeObservation: Boolean(options.includeObservation),
        maxCandidates: options.maxCandidates as number | undefined,
        waitAfterNavigationMs: options.waitAfterNavigationMs as number | undefined,
        observationOptions: {
          maxTextChars: options.maxTextChars as number | undefined,
          maxElements: options.maxElements as number | undefined
        },
        trace: Boolean(options.trace)
      });

      const json = JSON.stringify(result, null, 2);
      if (options.output) {
        await writeTextFile(options.output as string, json);
      }
      if (options.actionsOut) {
        const plan = planActionsFromInspectResult(result, {
          goal: options.goal as string | undefined,
          maxActions: options.planMaxActions as number | undefined,
          fillValue: options.fillValue as string | undefined
        });
        await writeTextFile(options.actionsOut as string, JSON.stringify(plan.actions, null, 2));
      }
      console.log(json);
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });


program
  .command("plan")
  .description("Generate runnable session actions from an inspect result JSON file")
  .argument("<inspect-result>", "Path to a JSON file produced by solarium inspect")
  .option("-o, --output <path>", "Write generated AgentAction[] JSON to a file")
  .option("--plan-output <path>", "Write the full action plan JSON to a file")
  .option("--goal <text>", "Optional planning goal used to rank inspect candidates")
  .option("--max-actions <number>", "Maximum actions to include", parseInteger, 10)
  .option("--fill-value <text>", "Value to use for generated fill/type actions")
  .option("--no-navigate", "Do not include an initial navigate action")
  .option("--no-observe", "Do not include a trailing observe action")
  .option("--screenshot <path>", "Add a screenshot action at the end of the plan")
  .option("--prefer-kind <kinds>", "Comma-separated candidate kinds to prefer: link,button,input,form,navigation")
  .action(async (inspectResultPath: string, options: Record<string, unknown>) => {
    try {
      const inspectResult = await readInspectResultFile(inspectResultPath);
      const plan = planActionsFromInspectResult(inspectResult, {
        goal: options.goal as string | undefined,
        maxActions: options.maxActions as number | undefined,
        fillValue: options.fillValue as string | undefined,
        includeNavigate: options.navigate as boolean | undefined,
        includeObserve: options.observe as boolean | undefined,
        includeScreenshot: Boolean(options.screenshot),
        screenshotPath: options.screenshot as string | undefined,
        preferKinds: parsePreferKinds(options.preferKind as string | undefined)
      });

      const actionsJson = JSON.stringify(plan.actions, null, 2);
      if (options.output) {
        await writeTextFile(options.output as string, actionsJson);
      }
      if (options.planOutput) {
        await writeTextFile(options.planOutput as string, JSON.stringify(plan, null, 2));
      }
      console.log(actionsJson);
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });


program
  .command("loop")
  .description("Run a bounded inspect-plan-act loop on a scoped page")
  .argument("<url>", "Starting URL")
  .option("--scope <path>", "Path to a JSON scope policy file")
  .option("-o, --output <path>", "Write the loop result JSON to a file")
  .option("--report <path>", "Write a Markdown loop report to a file")
  .option("--html-report <path>", "Write an HTML loop report to a file")
  .option("--report-include-json", "Include the full JSON result as a report appendix")
  .option("--events <path>", "Write a JSONL loop event timeline")
  .option("-e, --engine <engine>", "Browser engine: chromium, firefox, or webkit", "chromium")
  .option("-p, --profile <profile>", "Browser profile", "chrome-stable")
  .option("--profile-file <path>", "Path to a custom browser profile JSON file")
  .option("--storage-state <path>", "Load Playwright browser context storage state from a JSON file")
  .option("--save-storage-state <path>", "Save browser context storage state to a JSON file before closing")
  .option("--auth-session <path>", "Path to a Solarium auth-session profile JSON file")
  .option("--downloads-dir <path>", "Directory where browser downloads should be accepted and stored")
  .option("--headed", "Run with a visible browser window")
  .option("--loop-id <id>", "Stable loop identifier")
  .option("--goal <text>", "Goal text used to rank candidates each iteration")
  .option("--max-iterations <number>", "Maximum inspect-plan-act iterations", parseInteger, 5)
  .option("--actions-per-iteration <number>", "Maximum planned actions to execute per iteration", parseInteger, 1)
  .option("--max-candidates <number>", "Maximum inspect candidates per iteration", parseInteger, 100)
  .option("--stop-after-no-actions <number>", "Stop after this many consecutive iterations produce no actions", parseInteger, 1)
  .option("--stop-when-text <text>", "Stop when observed visible text contains this value")
  .option("--stop-when-url <pattern>", "Stop when current URL contains/matches this value; supports * wildcards or /regex/")
  .option("--stop-when-selector <selector>", "Stop when a selector exists on the page")
  .option("--continue-on-error", "Continue after a failed iteration")
  .option("--evidence-dir <path>", "Directory for generated loop evidence")
  .option("--screenshots", "Capture a screenshot after each iteration")
  .option("--include-observations", "Embed full observations in the loop result JSON")
  .option("--fill-value <text>", "Value to use for generated fill/type actions")
  .option("--wait-after-navigation-ms <number>", "Delay after initial navigation before looping", parseInteger)
  .option("--wait-after-action-ms <number>", "Delay after each generated action", parseInteger)
  .option("--max-text-chars <number>", "Maximum observed visible-text characters", parseInteger)
  .option("--max-elements <number>", "Maximum links/buttons/inputs/forms to observe", parseInteger)
  .option("--trace", "Record a Playwright trace")
  .action(async (url: string, options: Record<string, unknown>) => {
    try {
      const scope = await readOptionalScopePolicy(options.scope as string | undefined);
      const result = await runLoop({
        url,
        scope,
        engine: options.engine as BrowserEngine,
        profile: await resolveCliProfile(options),
        headless: !options.headed,
        storageState: (await resolveCliAuthSession(options)).storageState,
        saveStorageState: (await resolveCliAuthSession(options)).saveStorageState,
        downloadsDir: options.downloadsDir as string | undefined,
        loopId: options.loopId as string | undefined,
        goal: options.goal as string | undefined,
        maxIterations: options.maxIterations as number | undefined,
        actionsPerIteration: options.actionsPerIteration as number | undefined,
        maxCandidates: options.maxCandidates as number | undefined,
        stopAfterNoActions: options.stopAfterNoActions as number | undefined,
        stopWhenText: options.stopWhenText as string | undefined,
        stopWhenUrl: options.stopWhenUrl as string | undefined,
        stopWhenSelector: options.stopWhenSelector as string | undefined,
        continueOnError: Boolean(options.continueOnError),
        evidenceDir: options.evidenceDir as string | undefined,
        screenshotEachIteration: Boolean(options.screenshots),
        includeObservations: Boolean(options.includeObservations),
        fillValue: options.fillValue as string | undefined,
        waitAfterNavigationMs: options.waitAfterNavigationMs as number | undefined,
        waitAfterActionMs: options.waitAfterActionMs as number | undefined,
        observationOptions: {
          maxTextChars: options.maxTextChars as number | undefined,
          maxElements: options.maxElements as number | undefined
        },
        trace: Boolean(options.trace),
        eventLogger: createJsonlEventLogger(options.events as string | undefined)
      });

      const json = JSON.stringify(result, null, 2);
      if (options.output) {
        await writeTextFile(options.output as string, json);
      }
      if (options.report) {
        await writeTextFile(
          options.report as string,
          renderLoopMarkdownReport(result, { includeJsonAppendix: Boolean(options.reportIncludeJson) })
        );
      }
      if (options.htmlReport) {
        await writeTextFile(
          options.htmlReport as string,
          renderLoopHtmlReport(result, { includeJsonAppendix: Boolean(options.reportIncludeJson) })
        );
      }
      console.log(json);
      if (!result.ok) process.exitCode = 1;
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });


program
  .command("profiles")
  .description("List built-in browser profiles")
  .option("--json", "Output full profile summaries as JSON")
  .action(async (options: Record<string, unknown>) => {
    try {
      const profiles = listBuiltInProfiles();
      if (options.json) {
        console.log(JSON.stringify(profiles, null, 2));
        return;
      }
      for (const profile of profiles) {
        const viewport = profile.viewport ? `${profile.viewport.width}x${profile.viewport.height}` : "default";
        const mobile = profile.isMobile ? "mobile" : "desktop";
        console.log(`${profile.name}\t${mobile}\t${viewport}\t${profile.locale ?? ""}\t${profile.timezoneId ?? ""}`);
      }
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });

program
  .command("profiles:show")
  .alias("profile-show")
  .description("Show a built-in or custom browser profile")
  .argument("<profile>", "Built-in profile name or path to a profile JSON file")
  .option("--file", "Treat the argument as a custom profile JSON file")
  .action(async (profileRef: string, options: Record<string, unknown>) => {
    try {
      const profile = options.file ? await readBrowserProfile(profileRef) : getBuiltInProfile(profileRef) ?? await readBrowserProfile(profileRef);
      console.log(JSON.stringify(profile, null, 2));
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });

program
  .command("profiles:validate")
  .alias("profile-validate")
  .description("Validate a custom browser profile JSON file")
  .argument("<file>", "Path to a profile JSON file")
  .option("-o, --output <path>", "Write the validation result JSON to a file")
  .action(async (file: string, options: Record<string, unknown>) => {
    try {
      const parsed = JSON.parse(await readFile(file, "utf8")) as unknown;
      const result = validateBrowserProfile(parsed);
      const json = JSON.stringify({ ...result, path: file, summary: result.ok ? summarizeProfile(parsed as BrowserProfile, false) : undefined }, null, 2);
      if (options.output) await writeTextFile(options.output as string, json);
      console.log(json);
      if (!result.ok) process.exitCode = 1;
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });

program
  .command("validate")
  .description("Validate a Solarium job, scope policy, or actions JSON file")
  .argument("<file>", "Path to a JSON file to validate")
  .option("--kind <kind>", "Validation kind: auto, job, scope, or actions", "auto")
  .option("-o, --output <path>", "Write the validation result JSON to a file")
  .action(async (file: string, options: Record<string, unknown>) => {
    try {
      const kind = String(options.kind ?? "auto") as SolariumValidationKind;
      if (!["auto", "job", "scope", "actions"].includes(kind)) {
        throw new Error(`Unsupported validation kind: ${kind}`);
      }
      const result = await validateSolariumFile(file, kind);
      const json = JSON.stringify(result, null, 2);
      if (options.output) {
        await writeTextFile(options.output as string, json);
      }
      console.log(json);
      if (!result.ok) process.exitCode = 1;
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });

program
  .command("run")
  .description("Run a reproducible Solarium job from a JSON config file")
  .argument("<job>", "Path to a Solarium job JSON file")
  .option("-o, --output <path>", "Override the job output path")
  .action(async (jobPath: string, options: Record<string, unknown>) => {
    try {
      const job = await readSolariumJob(jobPath);
      if (options.output) {
        job.output = options.output as string;
      }
      const run = await runJob(job, { jobPath });
      console.log(JSON.stringify(run, null, 2));
      if (!run.ok) process.exitCode = 1;
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });

program
  .command("replay")
  .description("Summarize a Solarium JSONL event timeline")
  .requiredOption("--events <path>", "Path to a JSONL event timeline")
  .option("-o, --output <path>", "Write the replay summary JSON to a file")
  .action(async (options: Record<string, unknown>) => {
    try {
      const summary = await replayEvents(options.events as string);
      const json = JSON.stringify(summary, null, 2);
      if (options.output) {
        await writeTextFile(options.output as string, json);
      }
      console.log(json);
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });


program
  .command("auth-session")
  .description("Create or inspect Solarium auth-session profiles that reference Playwright storage-state files")
  .option("--create <path>", "Write a new auth-session profile JSON file")
  .option("--show <path>", "Read and print an auth-session profile JSON file")
  .option("--name <name>", "Profile name when creating")
  .option("--storage-state <path>", "Playwright storage-state JSON path when creating")
  .option("--description <text>", "Human-readable profile description")
  .option("--secret-ref <ref...>", "Secret reference identifiers associated with the session; never plaintext secrets")
  .action(async (options: Record<string, unknown>) => {
    try {
      if (options.show) {
        console.log(JSON.stringify(await readAuthSessionProfile(options.show as string), null, 2));
        return;
      }
      if (options.create) {
        const profile = await createAuthSessionProfile({
          output: options.create as string,
          name: options.name as string,
          storageState: options.storageState as string,
          description: options.description as string | undefined,
          secretRefs: options.secretRef as string[] | undefined
        });
        console.log(JSON.stringify(profile, null, 2));
        return;
      }
      throw new Error("Use --create <path> or --show <path>");
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });

program
  .command("scope-check")
  .description("Validate whether a URL is allowed by a JSON scope policy file")
  .requiredOption("--scope <path>", "Path to a JSON scope policy file")
  .argument("<url>", "URL to check")
  .action(async (url: string, options: Record<string, unknown>) => {
    try {
      const scope = await readScopePolicy(options.scope as string);
      const decision = checkUrlScope(url, scope);
      console.log(JSON.stringify(decision, null, 2));
      if (!decision.allowed) {
        process.exitCode = 1;
      }
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });


program
  .command("server")
  .description("Run a stdio JSON-RPC/MCP-style server for external agents")
  .action(async () => {
    try {
      await runJsonRpcServer();
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });



program
  .command("skill-seed")
  .description("Generate a Skiller-style workflow seed Markdown file from Solarium actions and optional evidence")
  .requiredOption("-a, --actions <path>", "Path to a JSON file containing AgentAction[]")
  .option("--evidence <path>", "Path to a solarium.evidence.v1 manifest")
  .option("-o, --output <path>", "Write the generated Markdown seed to a file")
  .option("--name <name>", "Workflow seed title")
  .option("--description <text>", "Workflow seed purpose text")
  .option("--source <source>", "Source label/path to include in the seed")
  .action(async (options: Record<string, unknown>) => {
    try {
      const markdown = await createWorkflowSeedFromFiles({
        actionsPath: options.actions as string,
        evidencePath: options.evidence as string | undefined,
        output: options.output as string | undefined,
        name: options.name as string | undefined,
        description: options.description as string | undefined,
        source: options.source as string | undefined
      });
      console.log(markdown);
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });

program
  .command("manifest")
  .description("Create a SHA-256 artifact manifest for Solarium evidence/report directories")
  .argument("<paths...>", "Evidence files or directories to inventory")
  .option("-o, --output <path>", "Write the manifest JSON to a file")
  .option("--include-hidden", "Include hidden files/directories below the selected roots")
  .option("--max-file-bytes <number>", "Skip hashing files larger than this many bytes", parseInteger)
  .option("--evidence", "Create a standardized Solarium evidence run manifest instead of a plain artifact manifest")
  .option("--run-id <id>", "Evidence run identifier")
  .option("--kind <kind>", "Evidence run kind: browse, inspect, session, loop, crawl, audit, replay, or manual", "manual")
  .option("--status <status>", "Evidence run status: ok, error, or partial")
  .option("--url <url>", "Evidence target URL")
  .option("--title <title>", "Evidence target title")
  .action(async (paths: string[], options: Record<string, unknown>) => {
    try {
      const manifest = options.evidence
        ? await createEvidenceRunManifest({
            roots: paths,
            output: options.output as string | undefined,
            includeHidden: Boolean(options.includeHidden),
            maxFileBytes: options.maxFileBytes as number | undefined,
            runId: (options.runId as string | undefined) ?? `run-${Date.now()}`,
            kind: ((options.kind as string | undefined) ?? "manual") as EvidenceRunKind,
            status: options.status as "ok" | "error" | "partial" | undefined,
            url: options.url as string | undefined,
            title: options.title as string | undefined
          })
        : await createArtifactManifest({
            roots: paths,
            output: options.output as string | undefined,
            includeHidden: Boolean(options.includeHidden),
            maxFileBytes: options.maxFileBytes as number | undefined
          });

      console.log(JSON.stringify(manifest, null, 2));
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  });

program.parseAsync();

function parseOwaspProfile(value?: string): OwaspAuditProfile {
  const profile = value ?? "passive";
  if (profile !== "passive" && profile !== "strict-headers" && profile !== "active-authorized" && profile !== "top10-passive" && profile !== "top10-active-authorized") {
    throw new Error(`Unsupported OWASP audit profile: ${profile}`);
  }
  return profile;
}

function parseInteger(value: string): number {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isFinite(parsed) || parsed < 0) {
    throw new Error(`Expected a non-negative integer, got: ${value}`);
  }
  return parsed;
}

async function readActionsFile(path: string): Promise<AgentAction[]> {
  const raw = await readFile(path, "utf8");
  const parsed = JSON.parse(raw) as unknown;

  if (!Array.isArray(parsed)) {
    throw new Error("Actions file must contain a JSON array");
  }

  return parsed.map(validateAction);
}


async function readInspectResultFile(path: string): Promise<InspectResult> {
  const raw = await readFile(path, "utf8");
  const parsed = JSON.parse(raw) as unknown;
  if (!parsed || typeof parsed !== "object") {
    throw new Error("Inspect result must be a JSON object");
  }
  const candidate = parsed as InspectResult;
  if (!Array.isArray(candidate.candidates)) {
    throw new Error("Inspect result must contain a candidates array");
  }
  return candidate;
}

function parsePreferKinds(value?: string): InspectResult["candidates"][number]["kind"][] | undefined {
  if (!value) return undefined;
  const allowed = new Set(["link", "button", "input", "form", "navigation"]);
  const kinds = value.split(",").map((kind) => kind.trim()).filter(Boolean);
  for (const kind of kinds) {
    if (!allowed.has(kind)) {
      throw new Error(`Unsupported candidate kind: ${kind}`);
    }
  }
  return kinds as InspectResult["candidates"][number]["kind"][];
}

async function resolveCliProfile(options: Record<string, unknown>): Promise<BrowserProfileName | BrowserProfile> {
  if (options.profileFile) {
    return readBrowserProfile(options.profileFile as string);
  }
  return (options.profile as BrowserProfileName | undefined) ?? "chrome-stable";
}


async function resolveCliAuthSession(options: Record<string, unknown>): Promise<{ storageState?: string; saveStorageState?: string }> {
  return resolveAuthSession({
    profilePath: options.authSession as string | undefined,
    storageState: options.storageState as string | undefined,
    saveStorageState: options.saveStorageState as string | undefined
  });
}

async function readOptionalScopePolicy(path?: string): Promise<ScopePolicy | undefined> {
  if (!path) return undefined;
  return readScopePolicy(path);
}

async function readScopePolicy(path: string): Promise<ScopePolicy> {
  const raw = await readFile(path, "utf8");
  return validateScopePolicy(JSON.parse(raw) as unknown);
}

function validateAction(action: unknown, index: number): AgentAction {
  if (!action || typeof action !== "object" || !("type" in action)) {
    throw new Error(`Action ${index} must be an object with a type field`);
  }

  const candidate = action as Record<string, unknown>;
  switch (candidate.type) {
    case "navigate":
      requireString(candidate.url, `Action ${index}.url`);
      return candidate as AgentAction;
    case "click":
    case "dblclick":
    case "hover":
      requireString(candidate.selector, `Action ${index}.selector`);
      return candidate as AgentAction;
    case "type":
      requireString(candidate.selector, `Action ${index}.selector`);
      requireString(candidate.text, `Action ${index}.text`);
      return candidate as AgentAction;
    case "press":
      requireString(candidate.selector, `Action ${index}.selector`);
      requireString(candidate.key, `Action ${index}.key`);
      return candidate as AgentAction;
    case "select":
      requireString(candidate.selector, `Action ${index}.selector`);
      if (Array.isArray(candidate.values)) {
        candidate.values.forEach((value, valueIndex) => requireString(value, `Action ${index}.values[${valueIndex}]`));
      } else {
        requireString(candidate.values, `Action ${index}.values`);
      }
      return candidate as AgentAction;
    case "check":
    case "uncheck":
    case "submit":
      requireString(candidate.selector, `Action ${index}.selector`);
      return candidate as AgentAction;
    case "upload":
      requireString(candidate.selector, `Action ${index}.selector`);
      if (Array.isArray(candidate.files)) {
        candidate.files.forEach((file, fileIndex) => requireString(file, `Action ${index}.files[${fileIndex}]`));
      } else {
        requireString(candidate.files, `Action ${index}.files`);
      }
      return candidate as AgentAction;
    case "download":
      requireString(candidate.selector, `Action ${index}.selector`);
      if (candidate.path !== undefined) requireString(candidate.path, `Action ${index}.path`);
      if (candidate.timeoutMs !== undefined) requireNonNegativeNumber(candidate.timeoutMs, `Action ${index}.timeoutMs`);
      return candidate as AgentAction;
    case "wait":
      requireNonNegativeNumber(candidate.ms, `Action ${index}.ms`);
      return candidate as AgentAction;
    case "waitForSelector":
      requireString(candidate.selector, `Action ${index}.selector`);
      if (candidate.state !== undefined && !["attached", "detached", "visible", "hidden"].includes(String(candidate.state))) {
        throw new Error(`Action ${index}.state must be attached, detached, visible, or hidden`);
      }
      if (candidate.timeoutMs !== undefined) requireNonNegativeNumber(candidate.timeoutMs, `Action ${index}.timeoutMs`);
      return candidate as AgentAction;
    case "waitForUrl":
      requireString(candidate.url, `Action ${index}.url`);
      if (candidate.timeoutMs !== undefined) requireNonNegativeNumber(candidate.timeoutMs, `Action ${index}.timeoutMs`);
      return candidate as AgentAction;
    case "screenshot":
    case "extract":
    case "observe":
      return candidate as AgentAction;
    default:
      throw new Error(`Unsupported action type at index ${index}: ${String(candidate.type)}`);
  }
}

function requireString(value: unknown, label: string): void {
  if (typeof value !== "string" || !value) {
    throw new Error(`${label} must be a non-empty string`);
  }
}

function requireNonNegativeNumber(value: unknown, label: string): void {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
    throw new Error(`${label} must be a non-negative number`);
  }
}

async function writeTextFile(path: string, content: string): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, content, "utf8");
}

import { lex, TokenStream } from "./lexer.js";
import { UsrlError } from "./errors.js";

export interface PromptFieldMap extends Record<string, unknown> {
  Id?: unknown;
  Title?: unknown;
  Purpose?: unknown;
  Type?: unknown;
  Inputs?: unknown;
  Outputs?: unknown;
  Body?: unknown;
  Links?: unknown;
}

export interface ContractFieldMap extends Record<string, unknown> {
  Id?: unknown;
  Title?: unknown;
  Purpose?: unknown;
  Scope?: unknown;
  Rules?: unknown;
  Targets?: unknown;
  Validation?: unknown;
  Links?: unknown;
}

export interface PromptUnit {
  id: string;
  fields: PromptFieldMap;
  links: Array<{ type?: string; target?: string; blocking?: boolean }>;
}

export interface ContractUnit {
  id: string;
  fields: ContractFieldMap;
  links: Array<{ type?: string; target?: string; blocking?: boolean }>;
}

export interface ParsedPll {
  metadata: Record<string, unknown>;
  prompts: PromptUnit[];
}

export interface ParsedCll {
  metadata: Record<string, unknown>;
  contracts: ContractUnit[];
}

export interface PairIssue {
  code: string;
  message: string;
}

const REQUIRED_META = ["Id", "Name", "Version", "Created", "Modified", "Author", "Source", "Purpose"];
const REQUIRED_PROMPT_FIELDS = ["Id", "Title", "Purpose", "Type", "Inputs", "Outputs", "Body", "Links"];
const REQUIRED_CONTRACT_FIELDS = ["Id", "Title", "Purpose", "Scope", "Rules", "Targets", "Validation", "Links"];

function parseValue(stream: TokenStream): unknown {
  const token = stream.peek();

  if (token.value === "{") {
    stream.next();
    const obj: Record<string, unknown> = {};
    while (!stream.matchValue("}")) {
      const key = stream.expectIdentifier("Expected object key").value;
      stream.expectValue(":", "Expected ':' after object key");
      obj[key] = parseValue(stream);
      stream.matchValue(";");
      stream.matchValue(",");
    }
    return obj;
  }

  if (token.value === "[") {
    stream.next();
    const values: unknown[] = [];
    while (!stream.matchValue("]")) {
      values.push(parseValue(stream));
      stream.matchValue(";");
      stream.matchValue(",");
    }
    return values;
  }

  if (token.type === "string") {
    return stream.next().value;
  }

  if (token.type === "number") {
    const raw = stream.next().value;
    const n = Number(raw);
    return Number.isNaN(n) ? raw : n;
  }

  if (token.value === "true" || token.value === "false") {
    stream.next();
    return token.value === "true";
  }

  if (token.type === "identifier" || token.type === "keyword") {
    return stream.next().value;
  }

  throw new UsrlError("SYNTAX_ERROR", `Unexpected token in value: ${token.value}`, token.line, token.column);
}

function parseMetadata(stream: TokenStream): Record<string, unknown> {
  stream.expectValue("metadata", "Expected metadata block");
  stream.expectValue("{", "Expected '{' after metadata");
  const out: Record<string, unknown> = {};
  while (!stream.matchValue("}")) {
    const key = stream.expectIdentifier("Expected metadata key").value;
    stream.expectValue(":", "Expected ':' after metadata key");
    out[key] = parseValue(stream);
    stream.expectValue(";", "Expected ';' after metadata field");
  }
  return out;
}

function expectLibraryStart(stream: TokenStream, className: "PromptLibrary" | "ContractLibrary"): void {
  stream.expectValue("version", "Expected version declaration");
  const versionToken = stream.peek();
  if (versionToken.type !== "string") {
    throw new UsrlError("SYNTAX_ERROR", "Version must be a string literal", versionToken.line, versionToken.column);
  }
  stream.next();
  stream.expectValue(";", "Expected ';' after version");

  if (stream.peek().value === "namespace") {
    stream.next();
    stream.expectIdentifier("Expected namespace name");
    while (stream.matchValue(".")) {
      stream.expectIdentifier("Expected namespace segment");
    }
    stream.expectValue("{", "Expected '{' for namespace");
    let depth = 1;
    while (!stream.atEnd() && depth > 0) {
      const t = stream.next();
      if (t.value === "{") depth += 1;
      if (t.value === "}") depth -= 1;
    }
  }

  stream.expectValue("library", "Expected library declaration");
  stream.expectValue(className, `Expected library ${className}`);
  stream.expectIdentifier("Expected library name");
  stream.expectValue("{", "Expected library body");
}

function asRecord(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function parseLinkList(value: unknown): Array<{ type?: string; target?: string; blocking?: boolean }> {
  if (!Array.isArray(value)) {
    return [];
  }

  return value.map((link) => {
    const obj = asRecord(link);
    return {
      type: typeof obj.Type === "string" ? obj.Type : undefined,
      target: typeof obj.Target === "string" ? obj.Target : undefined,
      blocking: typeof obj.Blocking === "boolean" ? obj.Blocking : undefined,
    };
  });
}

function parseUnitFields(stream: TokenStream, itemType: string): Record<string, unknown> {
  stream.expectValue("{", `Expected ${itemType} body`);
  const fields: Record<string, unknown> = {};
  while (!stream.matchValue("}")) {
    const key = stream.expectIdentifier(`Expected ${itemType} field`).value;
    stream.expectValue(":", `Expected ':' after ${itemType} field`);
    fields[key] = parseValue(stream);
    stream.expectValue(";", `Expected ';' after ${itemType} field`);
  }
  return fields;
}

function skipOptionalDecl(stream: TokenStream): void {
  stream.next();
  if (stream.peek().value === "{") {
    stream.next();
    let depth = 1;
    while (!stream.atEnd() && depth > 0) {
      const t = stream.next();
      if (t.value === "{") depth += 1;
      if (t.value === "}") depth -= 1;
    }
  } else {
    while (!stream.atEnd() && !stream.matchValue(";")) {
      if (stream.peek().value === "{") {
        break;
      }
      stream.next();
    }
  }
}

export function parsePll(source: string): ParsedPll {
  const stream = new TokenStream(lex(source));
  expectLibraryStart(stream, "PromptLibrary");

  const metadata = parseMetadata(stream);
  const prompts: PromptUnit[] = [];

  while (!stream.matchValue("}")) {
    const token = stream.peek();
    if (token.type === "eof") {
      throw new UsrlError("SYNTAX_ERROR", "Unterminated PromptLibrary block", token.line, token.column);
    }

    if (token.value === "prompt") {
      stream.next();
      stream.expectIdentifier("Expected prompt declaration name");
      const fields = parseUnitFields(stream, "prompt") as PromptFieldMap;
      prompts.push({
        id: String(fields.Id ?? ""),
        fields,
        links: parseLinkList(fields.Links),
      });
      continue;
    }

    skipOptionalDecl(stream);
  }

  return { metadata, prompts };
}

export function parseCll(source: string): ParsedCll {
  const stream = new TokenStream(lex(source));
  expectLibraryStart(stream, "ContractLibrary");

  const metadata = parseMetadata(stream);
  const contracts: ContractUnit[] = [];

  while (!stream.matchValue("}")) {
    const token = stream.peek();
    if (token.type === "eof") {
      throw new UsrlError("SYNTAX_ERROR", "Unterminated ContractLibrary block", token.line, token.column);
    }

    if (token.value === "contract_unit") {
      stream.next();
      stream.expectIdentifier("Expected contract declaration name");
      const fields = parseUnitFields(stream, "contract") as ContractFieldMap;
      contracts.push({
        id: String(fields.Id ?? ""),
        fields,
        links: parseLinkList(fields.Links),
      });
      continue;
    }

    skipOptionalDecl(stream);
  }

  return { metadata, contracts };
}

function isString(value: unknown): value is string {
  return typeof value === "string";
}

function isArray(value: unknown): value is unknown[] {
  return Array.isArray(value);
}

function validateLinks(links: Array<{ type?: string; target?: string }>, owner: string, issues: PairIssue[]): void {
  for (let i = 0; i < links.length; i += 1) {
    const link = links[i];
    if (!link.type) {
      issues.push({ code: "SEMANTIC_ERROR", message: `${owner}.Links[${i}] missing Type` });
    }
    if (!link.target) {
      issues.push({ code: "SEMANTIC_ERROR", message: `${owner}.Links[${i}] missing Target` });
    }
  }
}

export function validatePll(pll: ParsedPll): PairIssue[] {
  const issues: PairIssue[] = [];

  for (const key of REQUIRED_META) {
    if (!(key in pll.metadata)) {
      issues.push({ code: "SEMANTIC_ERROR", message: `.pll metadata missing required field '${key}'` });
    }
  }

  if (pll.prompts.length === 0) {
    issues.push({ code: "SEMANTIC_ERROR", message: ".pll must contain at least one prompt" });
  }

  const seen = new Set<string>();
  for (const prompt of pll.prompts) {
    if (!prompt.id) {
      issues.push({ code: "SEMANTIC_ERROR", message: "Prompt.Id must be non-empty" });
      continue;
    }
    if (seen.has(prompt.id)) {
      issues.push({ code: "SEMANTIC_ERROR", message: `Duplicate prompt Id '${prompt.id}'` });
    }
    seen.add(prompt.id);

    for (const field of REQUIRED_PROMPT_FIELDS) {
      if (!(field in prompt.fields)) {
        issues.push({ code: "SEMANTIC_ERROR", message: `Prompt '${prompt.id}' missing required field '${field}'` });
      }
    }

    if ("Title" in prompt.fields && !isString(prompt.fields.Title)) {
      issues.push({ code: "SEMANTIC_ERROR", message: `Prompt '${prompt.id}' field 'Title' must be string` });
    }
    if ("Purpose" in prompt.fields && !isString(prompt.fields.Purpose)) {
      issues.push({ code: "SEMANTIC_ERROR", message: `Prompt '${prompt.id}' field 'Purpose' must be string` });
    }
    if ("Type" in prompt.fields && !isString(prompt.fields.Type)) {
      issues.push({ code: "SEMANTIC_ERROR", message: `Prompt '${prompt.id}' field 'Type' must be string` });
    }
    if ("Body" in prompt.fields && !isString(prompt.fields.Body)) {
      issues.push({ code: "SEMANTIC_ERROR", message: `Prompt '${prompt.id}' field 'Body' must be string` });
    }
    if ("Inputs" in prompt.fields && !isArray(prompt.fields.Inputs)) {
      issues.push({ code: "SEMANTIC_ERROR", message: `Prompt '${prompt.id}' field 'Inputs' must be array` });
    }
    if ("Outputs" in prompt.fields && !isArray(prompt.fields.Outputs)) {
      issues.push({ code: "SEMANTIC_ERROR", message: `Prompt '${prompt.id}' field 'Outputs' must be array` });
    }
    if ("Links" in prompt.fields && !isArray(prompt.fields.Links)) {
      issues.push({ code: "SEMANTIC_ERROR", message: `Prompt '${prompt.id}' field 'Links' must be array` });
    }

    validateLinks(prompt.links, `Prompt '${prompt.id}'`, issues);
  }

  return issues;
}

export function validateCll(cll: ParsedCll): PairIssue[] {
  const issues: PairIssue[] = [];

  for (const key of REQUIRED_META) {
    if (!(key in cll.metadata)) {
      issues.push({ code: "SEMANTIC_ERROR", message: `.cll metadata missing required field '${key}'` });
    }
  }

  if (cll.contracts.length === 0) {
    issues.push({ code: "SEMANTIC_ERROR", message: ".cll must contain at least one contract_unit" });
  }

  const seen = new Set<string>();
  for (const contract of cll.contracts) {
    if (!contract.id) {
      issues.push({ code: "SEMANTIC_ERROR", message: "Contract.Id must be non-empty" });
      continue;
    }
    if (seen.has(contract.id)) {
      issues.push({ code: "SEMANTIC_ERROR", message: `Duplicate contract Id '${contract.id}'` });
    }
    seen.add(contract.id);

    for (const field of REQUIRED_CONTRACT_FIELDS) {
      if (!(field in contract.fields)) {
        issues.push({ code: "SEMANTIC_ERROR", message: `Contract '${contract.id}' missing required field '${field}'` });
      }
    }

    if ("Title" in contract.fields && !isString(contract.fields.Title)) {
      issues.push({ code: "SEMANTIC_ERROR", message: `Contract '${contract.id}' field 'Title' must be string` });
    }
    if ("Purpose" in contract.fields && !isString(contract.fields.Purpose)) {
      issues.push({ code: "SEMANTIC_ERROR", message: `Contract '${contract.id}' field 'Purpose' must be string` });
    }
    if ("Scope" in contract.fields && !isString(contract.fields.Scope)) {
      issues.push({ code: "SEMANTIC_ERROR", message: `Contract '${contract.id}' field 'Scope' must be string` });
    }
    if ("Rules" in contract.fields && !isArray(contract.fields.Rules)) {
      issues.push({ code: "SEMANTIC_ERROR", message: `Contract '${contract.id}' field 'Rules' must be array` });
    }
    if ("Targets" in contract.fields && !isArray(contract.fields.Targets)) {
      issues.push({ code: "SEMANTIC_ERROR", message: `Contract '${contract.id}' field 'Targets' must be array` });
    }
    if ("Validation" in contract.fields && !isArray(contract.fields.Validation)) {
      issues.push({ code: "SEMANTIC_ERROR", message: `Contract '${contract.id}' field 'Validation' must be array` });
    }
    if ("Links" in contract.fields && !isArray(contract.fields.Links)) {
      issues.push({ code: "SEMANTIC_ERROR", message: `Contract '${contract.id}' field 'Links' must be array` });
    }

    const rules = contract.fields.Rules;
    if (isArray(rules) && rules.length === 0) {
      issues.push({ code: "SEMANTIC_ERROR", message: `Contract '${contract.id}' field 'Rules' must be non-empty` });
    }

    const validation = contract.fields.Validation;
    if (isArray(validation) && validation.length === 0) {
      issues.push({ code: "SEMANTIC_ERROR", message: `Contract '${contract.id}' field 'Validation' must be non-empty` });
    }

    validateLinks(contract.links, `Contract '${contract.id}'`, issues);
  }

  return issues;
}

export function validatePair(pll: ParsedPll, cll: ParsedCll): PairIssue[] {
  const issues: PairIssue[] = [...validatePll(pll), ...validateCll(cll)];

  const pllId = pll.metadata.Id;
  const cllId = cll.metadata.Id;
  if (typeof pllId === "string" && typeof cllId === "string" && pllId !== cllId) {
    issues.push({
      code: "SEMANTIC_ERROR",
      message: `Pair metadata Id mismatch: pll='${pllId}' cll='${cllId}'`,
    });
  }

  const promptIds = new Set(pll.prompts.map((p) => p.id));
  for (const contract of cll.contracts) {
    for (const link of contract.links) {
      if ((link.type === "Governs" || link.type === "Targets") && link.blocking === true) {
        if (!link.target || !promptIds.has(link.target)) {
          issues.push({
            code: "SEMANTIC_ERROR",
            message: `Blocking ${link.type} link from contract '${contract.id}' unresolved target '${link.target ?? ""}'`,
          });
        }
      }
    }
  }

  return issues;
}

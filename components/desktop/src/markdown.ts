import type { Message } from './types';
import { escapeHtml } from './html';

type MarkdownSegment =
  | { kind: 'text'; text: string }
  | { kind: 'code'; language: string; code: string };

const CODE_FENCE_RE = /```([^\n`]*)\n?([\s\S]*?)(?:```|$)/g;
const INLINE_CODE_RE = /`([^`]+)`/g;
type SyntaxKind =
  | 'comment'
  | 'keyword'
  | 'number'
  | 'string'
  | 'function'
  | 'type'
  | 'property'
  | 'builtin'
  | 'operator'
  | 'punctuation'
  | 'diff-add'
  | 'diff-delete'
  | 'diff-meta'
  | 'tag'
  | 'attr';

const COMMON_CODE_KEYWORDS = new Set([
  'abstract', 'as', 'async', 'await', 'bool', 'boolean', 'break', 'case', 'catch', 'char', 'class', 'const', 'continue', 'def', 'default',
  'do', 'else', 'enum', 'export', 'extends', 'false', 'final', 'finally', 'float', 'fn', 'for', 'from', 'function', 'if', 'impl',
  'import', 'in', 'int', 'interface', 'let', 'match', 'mod', 'mut', 'namespace', 'new', 'none', 'null', 'package', 'pass', 'private',
  'protected', 'public', 'return', 'self', 'static', 'struct', 'super', 'switch', 'this', 'throw', 'trait', 'true', 'try', 'type',
  'undefined', 'use', 'using', 'var', 'void', 'while', 'yield', 'where', 'with', 'readonly', 'record', 'sealed', 'virtual', 'override',
]);

const LANGUAGE_CODE_KEYWORDS: Record<string, string[]> = {
  csharp: ['base', 'decimal', 'delegate', 'dynamic', 'event', 'get', 'global', 'internal', 'is', 'lock', 'object', 'out', 'params', 'partial', 'ref', 'required', 'set', 'sizeof', 'stackalloc', 'string', 'typeof', 'uint', 'ulong', 'unchecked', 'unsafe'],
  css: ['and', 'from', 'important', 'media', 'not', 'or', 'supports', 'to'],
  html: ['doctype'],
  java: ['assert', 'byte', 'double', 'implements', 'instanceof', 'long', 'native', 'short', 'strictfp', 'synchronized', 'throws', 'transient', 'volatile'],
  javascript: ['debugger', 'delete', 'instanceof', 'of'],
  json: [],
  markdown: [],
  python: ['and', 'assert', 'del', 'elif', 'except', 'global', 'is', 'lambda', 'nonlocal', 'not', 'or', 'raise'],
  rust: ['crate', 'dyn', 'extern', 'loop', 'move', 'pub', 'ref', 'unsafe'],
  shell: ['alias', 'case', 'cd', 'done', 'elif', 'esac', 'eval', 'exec', 'exit', 'fi', 'local', 'read', 'then'],
  typescript: ['declare', 'keyof', 'satisfies', 'symbol', 'unique'],
  yaml: ['true', 'false', 'null'],
};

const CODE_BUILTINS = new Set([
  'Array', 'Boolean', 'Console', 'Date', 'Dict', 'Error', 'Exception', 'JSON', 'List', 'Map', 'Math', 'Number', 'Object', 'Promise', 'Record',
  'Regex', 'RegExp', 'Set', 'String', 'Vec', 'console', 'dict', 'enumerate', 'len', 'list', 'map', 'print', 'range', 'str', 'sum', 'println',
]);

export function renderMessage(message: Message): string {
  const role = message.role ?? 'message';
  const isUser = role === 'user';
  const isCommand = role === 'command';
  const isTool = role.includes('tool') || role.includes('event');
  const cardClass = isUser ? 'ml-auto max-w-3xl bg-white/[0.07]' : isCommand ? 'max-w-4xl border-vv-cyan/35 bg-vv-cyan/5' : isTool ? 'max-w-4xl border-vv-line bg-black/18 opacity-75' : 'max-w-4xl bg-white/[0.035]';
  return `
    <article class="vv-soft-panel ${cardClass}">
      <header class="flex items-center gap-3 border-b border-vv-line px-4 py-2 text-[0.68rem] uppercase tracking-[0.22em] text-vv-muted"><span class="h-2 w-2 rounded-full ${isUser ? 'bg-vv-green' : isCommand ? 'bg-vv-cyan' : 'bg-vv-cyan'}"></span>${escapeHtml(role)}</header>
      <div class="px-4 py-3">${renderMessageMarkdown(message.content ?? message.text ?? '')}</div>
    </article>
  `;
}

function renderMessageMarkdown(markdown: string): string {
  const segments = parseMarkdownSegments(markdown);
  if (!segments.length) return '<div class="vv-chat-markdown text-vv-muted">(empty)</div>';
  return segments.map((segment) => segment.kind === 'code'
    ? renderCodeFence(segment.code, segment.language)
    : renderMarkdownText(segment.text)
  ).join('');
}

function parseMarkdownSegments(markdown: string): MarkdownSegment[] {
  const segments: MarkdownSegment[] = [];
  let cursor = 0;
  CODE_FENCE_RE.lastIndex = 0;
  for (const match of markdown.matchAll(CODE_FENCE_RE)) {
    const start = match.index ?? 0;
    if (start > cursor) segments.push({ kind: 'text', text: markdown.slice(cursor, start) });
    segments.push({ kind: 'code', language: normalizeCodeLanguage(match[1] ?? ''), code: stripTrailingFenceNewline(match[2] ?? '') });
    cursor = start + match[0].length;
  }
  if (cursor < markdown.length) segments.push({ kind: 'text', text: markdown.slice(cursor) });
  return segments.filter((segment) => segment.kind === 'code' || segment.text.length > 0);
}

function stripTrailingFenceNewline(code: string): string {
  return code.replace(/^\n/, '').replace(/\n$/, '');
}

function normalizeCodeLanguage(language: string): string {
  const normalized = language.trim().split(/\s+/)[0]?.toLowerCase() ?? '';
  const aliases: Record<string, string> = {
    csharp: 'csharp', cs: 'csharp', js: 'javascript', jsx: 'javascript', py: 'python', rs: 'rust', sh: 'shell', bash: 'shell',
    ts: 'typescript', tsx: 'typescript', yml: 'yaml', md: 'markdown', diff: 'diff', patch: 'diff', plaintext: 'text', plain: 'text',
  };
  return aliases[normalized] ?? normalized;
}

function renderMarkdownText(text: string): string {
  if (!text.trim()) return '';
  const blocks = text.replace(/^\n+|\n+$/g, '').split(/\n{2,}/);
  return blocks.map((block) => `<div class="vv-chat-markdown">${renderInlineMarkdown(block).replaceAll('\n', '<br />')}</div>`).join('');
}

function renderInlineMarkdown(text: string): string {
  return escapeHtml(text).replace(INLINE_CODE_RE, (_match, code: string) => `<code class="vv-chat-inline-code">${code}</code>`);
}

function renderCodeFence(code: string, language: string): string {
  const label = language || 'code';
  return `
    <section class="vv-chat-code-block">
      <div class="vv-chat-code-header"><span>${escapeHtml(label)}</span></div>
      <pre class="vv-code vv-scrollbar"><code class="language-${escapeHtml(label)}">${highlightCode(code, language)}</code></pre>
    </section>`;
}

function highlightCode(code: string, language: string): string {
  const mode = normalizeCodeLanguage(language);
  if (mode === 'diff') return highlightDiffCode(code);
  if (mode === 'html' || mode === 'xml') return highlightMarkupCode(code);

  const keywords = keywordsForLanguage(mode);
  let output = '';
  for (let index = 0; index < code.length;) {
    const rest = code.slice(index);

    const blockComment = rest.match(/^\/\*[\s\S]*?\*\//);
    if (blockComment) {
      output += syntaxToken(blockComment[0], 'comment');
      index += blockComment[0].length;
      continue;
    }

    const lineComment = rest.match(lineCommentPattern(mode));
    if (lineComment) {
      output += syntaxToken(lineComment[0], 'comment');
      index += lineComment[0].length;
      continue;
    }

    const stringLiteral = rest.match(stringLiteralPattern(mode));
    if (stringLiteral) {
      const token = stringLiteral[0];
      const after = code.slice(index + token.length).match(/^\s*:/);
      output += (after && ['json', 'yaml'].includes(mode)) ? syntaxToken(token, 'property') : syntaxToken(token, 'string');
      index += token.length;
      continue;
    }

    const numberLiteral = rest.match(/^\b(?:0b[01_]+|0o[0-7_]+|0x[0-9a-fA-F_]+|\d[\d_]*(?:\.\d[\d_]*)?)(?:[eE][+-]?\d[\d_]*)?\b/);
    if (numberLiteral) {
      output += syntaxToken(numberLiteral[0], 'number');
      index += numberLiteral[0].length;
      continue;
    }

    const decorator = rest.match(/^@[A-Za-z_][A-Za-z0-9_.-]*/);
    if (decorator) {
      output += syntaxToken(decorator[0], 'attr');
      index += decorator[0].length;
      continue;
    }

    const identifier = rest.match(/^[A-Za-z_$][A-Za-z0-9_$]*/);
    if (identifier) {
      const token = identifier[0];
      const previous = code[index - 1] ?? '';
      const next = code.slice(index + token.length);
      if (keywords.has(token.toLowerCase())) output += syntaxToken(token, 'keyword');
      else if (CODE_BUILTINS.has(token)) output += syntaxToken(token, 'builtin');
      else if (previous === '.') output += syntaxToken(token, 'property');
      else if (/^\s*[:<]/.test(next) && /^[A-Z]/.test(token)) output += syntaxToken(token, 'type');
      else if (/^\s*\(/.test(next)) output += syntaxToken(token, 'function');
      else output += escapeHtml(token);
      index += token.length;
      continue;
    }

    const operator = rest.match(/^(=>|==={0,1}|!={1,2}|<=|>=|&&|\|\||::|->|\.\.=?|[+\-*/%=<>!&|^~?:]+)/);
    if (operator) {
      output += syntaxToken(operator[0], 'operator');
      index += operator[0].length;
      continue;
    }

    const punctuation = rest.match(/^[{}()[\],.;]/);
    if (punctuation) {
      output += syntaxToken(punctuation[0], 'punctuation');
      index += punctuation[0].length;
      continue;
    }

    output += escapeHtml(code[index]);
    index += 1;
  }
  return output;
}

function highlightDiffCode(code: string): string {
  return code.split('\n').map((line) => {
    const kind: SyntaxKind = line.startsWith('+') && !line.startsWith('+++') ? 'diff-add'
      : line.startsWith('-') && !line.startsWith('---') ? 'diff-delete'
        : /^(@@|diff |index |\+\+\+|---)/.test(line) ? 'diff-meta'
          : 'punctuation';
    return syntaxToken(line, kind);
  }).join('\n');
}

function highlightMarkupCode(code: string): string {
  return escapeHtml(code).replace(/(&lt;\/?)([A-Za-z][\w:-]*)([^&]*?)(\/?&gt;)/g, (_match, open: string, tag: string, attrs: string, close: string) =>
    `${open}<span class="vv-syntax-tag">${tag}</span>${highlightMarkupAttrs(attrs)}${close}`
  );
}

function highlightMarkupAttrs(attrs: string): string {
  return attrs.replace(/([A-Za-z_:][-A-Za-z0-9_:.]*)(=)(&quot;.*?&quot;|'.*?'|[^\s&]+)/g, (_match, name: string, eq: string, value: string) =>
    `<span class="vv-syntax-attr">${name}</span>${eq}<span class="vv-syntax-string">${value}</span>`
  );
}

function keywordsForLanguage(language: string): Set<string> {
  return new Set([...COMMON_CODE_KEYWORDS, ...(LANGUAGE_CODE_KEYWORDS[language] ?? [])].map((keyword) => keyword.toLowerCase()));
}

function stringLiteralPattern(language: string): RegExp {
  if (language === 'shell') return /^(?:'[^']*'|"(?:\\.|[^"\\])*"|`(?:\\.|[^`\\])*`)/;
  return /^(?:r#*"[\s\S]*?"#*|b?"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*')/;
}

function lineCommentPattern(language: string): RegExp {
  if (['python', 'shell', 'yaml'].includes(language)) return /^#[^\n]*/;
  return /^\/\/[^\n]*/;
}

function syntaxToken(value: string, kind: SyntaxKind): string {
  return `<span class="vv-syntax-${kind}">${escapeHtml(value)}</span>`;
}

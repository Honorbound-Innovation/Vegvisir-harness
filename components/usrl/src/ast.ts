export type TopDeclKind =
  | "using"
  | "import"
  | "namespace"
  | "library"
  | "definition"
  | "enum"
  | "struct"
  | "type"
  | "template"
  | "expand"
  | "contract";

export interface SourceLoc {
  line: number;
  column: number;
}

export type LiteralValue = string | number | boolean | null | "unknown";

export interface Pattern {
  kind: "identifier" | "tuple" | "object";
  name?: string;
  items?: Pattern[];
  fields?: Array<{ key: string; pattern: Pattern }>;
}

export interface CallArg {
  name?: string;
  value: Expr;
}

export interface MatchCase {
  pattern: Pattern;
  guard?: Expr;
  value: Expr;
}

export interface Expr {
  kind:
    | "literal"
    | "identifier"
    | "unary"
    | "binary"
    | "conditional"
    | "coalesce"
    | "call"
    | "member"
    | "safe_member"
    | "index"
    | "array"
    | "set"
    | "object"
    | "new"
    | "lambda"
    | "provenance"
    | "match"
    | "comprehension";
  value?: LiteralValue;
  name?: string;
  operator?: string;
  left?: Expr;
  right?: Expr;
  test?: Expr;
  consequent?: Expr;
  alternate?: Expr;
  callee?: Expr;
  args?: Expr[];
  callArgs?: CallArg[];
  object?: Expr;
  property?: string;
  index?: Expr;
  elements?: Expr[];
  fields?: Array<{ key: string; value: Expr }>;
  typeName?: string;
  source?: Expr;
  time?: Expr;
  confidence?: Expr;
  matchExpr?: Expr;
  cases?: MatchCase[];
  itemExpr?: Expr;
  pattern?: Pattern;
  iterable?: Expr;
  filter?: Expr;
}

export interface QuerySpec {
  target?: Expr;
  where?: Expr;
  constraints?: Expr;
  response_format?: Expr;
  hint?: Expr;
}

export interface TypeExpr {
  kind: "ref" | "union" | "variant" | "object";
  name?: string;
  qname?: string;
  args?: TypeExpr[];
  elementType?: TypeExpr;
  fields?: Array<{ key: string; type: TypeExpr }>;
  variants?: TypeExpr[];
}

export interface StructField {
  name: string;
  type: TypeExpr;
}

export interface ParamSpec {
  type?: string;
  name: string;
  defaultValue?: Expr;
}

export interface ImportSpec {
  sourcePath: string;
  alias?: string;
  names?: string[];
}

export interface EnumMember {
  name: string;
  value?: Expr;
}

export interface InheritanceSpec {
  relation: "extends" | "derives";
  target: string;
}

export interface TopDecl {
  kind: TopDeclKind;
  name?: string;
  loc: SourceLoc;
  body?: Statement[];
  declarations?: TopDecl[];
  typeParams?: string[];
  typeExpr?: TypeExpr;
  structFields?: StructField[];
  importSpec?: ImportSpec;
  enumMembers?: EnumMember[];
  inheritance?: InheritanceSpec;
  params?: ParamSpec[];
  args?: Expr[];
  callArgs?: CallArg[];
}

export interface Statement {
  kind:
    | "section"
    | "fact"
    | "rule"
    | "constraint"
    | "stage"
    | "trigger"
    | "let"
    | "assert"
    | "require"
    | "permit"
    | "deny"
    | "emit"
    | "query"
    | "when"
    | "if"
    | "foreach"
    | "parallel"
    | "sequence"
    | "return"
    | "expr";
  name?: string;
  expr?: Expr;
  pattern?: Pattern;
  iterable?: Expr;
  body?: Statement[];
  elseBody?: Statement[];
  query?: QuerySpec;
  params?: ParamSpec[];
  hints?: Expr[];
  loc: SourceLoc;
}

export interface Program {
  declarations: TopDecl[];
}

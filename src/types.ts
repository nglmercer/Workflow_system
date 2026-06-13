export interface ASTNode {
  type: string;
  [key: string]: unknown;
}

export interface FlowProgram {
  imports: ImportStmt[];
  functions: FnDef[];
  workflows: WorkflowDef[];
  stmts: Stmt[];
}

export interface ImportStmt {
  type: 'Import';
  name: string;
  path: string;
}

export interface FnDef {
  type: 'FnDef';
  name: string;
  params: string[];
  body: Stmt[];
}

export interface WorkflowDef {
  type: 'WorkflowDef';
  name: string;
  event: string;
  params: string[];
  body: Stmt[];
}

export type Stmt =
  | { type: 'VarDecl'; name: string; value: Expr }
  | { type: 'If'; condition: Expr; thenBody: Stmt[]; elseBody: Stmt[] | null }
  | { type: 'Log'; expr: Expr }
  | { type: 'Foreach'; itemVar: string; iterable: Expr; body: Stmt[] }
  | { type: 'On'; event: string; params: string[] }
  | { type: 'Return'; value: Expr }
  | { type: 'Emit'; workflow: string; data: Expr }
  | { type: 'Expr'; expr: Expr };

export type Expr =
  | { type: 'String'; value: string }
  | { type: 'Number'; value: number }
  | { type: 'Bool'; value: boolean }
  | { type: 'Null' }
  | { type: 'Var'; name: string }
  | { type: 'Member'; object: Expr; property: string }
  | { type: 'BinaryOp'; op: string; left: Expr; right: Expr }
  | { type: 'UnaryOp'; op: string; operand: Expr }
  | { type: 'Call'; name: string; args: Expr[] }
  | { type: 'Array'; elements: Expr[] };

export type TokenType =
  | 'keyword' | 'string' | 'number' | 'comment'
  | 'operator' | 'punctuation' | 'identifier'
  | 'function' | 'property' | 'type' | 'error';

export interface Token {
  type: TokenType;
  value: string;
  start: number;
  end: number;
}

export interface Diagnostic {
  severity: 'error' | 'warning' | 'info';
  message: string;
  line: number;
  column: number;
}

export interface CompletionItem {
  label: string;
  kind: 'keyword' | 'function' | 'variable' | 'property' | 'snippet';
  detail?: string;
  insertText: string;
}

import type { FlowProgram, FnDef, WorkflowDef, Stmt, Expr } from './types.ts';

export class Parser {
  private input: string;
  private pos = 0;

  constructor(input: string) {
    this.input = input;
  }

  private peek(): string | null {
    return this.pos < this.input.length ? this.input[this.pos] : null;
  }
  private advance(): string { return this.input[this.pos++]; }
  private eof(): boolean { return this.pos >= this.input.length; }

  private skip(): void {
    while (!this.eof()) {
      const c = this.peek()!;
      if (/\s/.test(c)) { this.pos++; continue; }
      if (c === '/' && this.input[this.pos + 1] === '/') {
        this.pos += 2;
        while (!this.eof() && this.peek() !== '\n') this.pos++;
        continue;
      }
      break;
    }
  }

  private at(s: string): boolean { this.skip(); return this.input.startsWith(s, this.pos); }

  private match(s: string): boolean {
    this.skip();
    if (this.input.startsWith(s, this.pos)) { this.pos += s.length; return true; }
    return false;
  }

  private expect(s: string): void {
    if (!this.match(s)) throw new Error(`Expected '${s}' near '${this.input.slice(this.pos, this.pos + 30)}'`);
  }

  private matchKeyword(kw: string): boolean {
    this.skip();
    const rest = this.input.slice(this.pos);
    if (rest.startsWith(kw) && !/[a-zA-Z0-9_]/.test(rest[kw.length] || '')) {
      this.pos += kw.length;
      return true;
    }
    return false;
  }

  private parseIdent(): string {
    this.skip();
    const start = this.pos;
    if (!/[a-zA-Z_]/.test(this.peek()!)) throw new Error(`Expected identifier near '${this.input.slice(this.pos, this.pos + 20)}'`);
    while (!this.eof() && /[a-zA-Z0-9_]/.test(this.peek()!)) this.pos++;
    return this.input.slice(start, this.pos);
  }

  private parseString(): string {
    this.skip();
    this.expect('"');
    let s = '';
    while (!this.eof() && this.peek() !== '"') {
      if (this.peek() === '\\') { this.pos++; s += this.advance(); }
      else s += this.advance();
    }
    this.expect('"');
    return s;
  }

  private parseNumber(): number {
    this.skip();
    const start = this.pos;
    if (this.peek() === '-') this.pos++;
    while (!this.eof() && /[0-9]/.test(this.peek()!)) this.pos++;
    if (!this.eof() && this.peek() === '.') {
      this.pos++;
      while (!this.eof() && /[0-9]/.test(this.peek()!)) this.pos++;
    }
    return parseFloat(this.input.slice(start, this.pos));
  }

  parseExpr(): Expr { return this.parseOr(); }

  private parseOr(): Expr {
    let left = this.parseAnd();
    while (this.match('||')) {
      left = { type: 'BinaryOp', op: '||', left, right: this.parseAnd() };
    }
    return left;
  }

  private parseAnd(): Expr {
    let left = this.parseComparison();
    while (this.match('&&')) {
      left = { type: 'BinaryOp', op: '&&', left, right: this.parseComparison() };
    }
    return left;
  }

  private parseComparison(): Expr {
    let left = this.parseAdditive();
    const ops = ['==', '!=', '<=', '>=', '<', '>'];
    this.skip();
    for (const op of ops) {
      if (this.input.startsWith(op, this.pos)) {
        this.pos += op.length;
        left = { type: 'BinaryOp', op, left, right: this.parseAdditive() };
        return left;
      }
    }
    return left;
  }

  private parseAdditive(): Expr {
    let left = this.parseMultiplicative();
    while (true) {
      this.skip();
      if (this.peek() === '+') { this.pos++; left = { type: 'BinaryOp', op: '+', left, right: this.parseMultiplicative() }; }
      else if (this.peek() === '-') { this.pos++; left = { type: 'BinaryOp', op: '-', left, right: this.parseMultiplicative() }; }
      else break;
    }
    return left;
  }

  private parseMultiplicative(): Expr {
    let left = this.parseUnary();
    while (true) {
      this.skip();
      if (this.peek() === '*') { this.pos++; left = { type: 'BinaryOp', op: '*', left, right: this.parseUnary() }; }
      else if (this.peek() === '/') { this.pos++; left = { type: 'BinaryOp', op: '/', left, right: this.parseUnary() }; }
      else break;
    }
    return left;
  }

  private parseUnary(): Expr {
    this.skip();
    if (this.peek() === '!') { this.pos++; return { type: 'UnaryOp', op: '!', operand: this.parseUnary() }; }
    if (this.peek() === '-') { this.pos++; return { type: 'UnaryOp', op: '-', operand: this.parseUnary() }; }
    return this.parsePrimary();
  }

  private parsePrimary(): Expr {
    this.skip();
    if (this.peek() === '"') return { type: 'String', value: this.parseString() };
    if (/[0-9]/.test(this.peek()!) || (this.peek() === '-' && /[0-9]/.test(this.input[this.pos + 1] || '')))
      return { type: 'Number', value: this.parseNumber() };
    if (this.peek() === '(') { this.pos++; const e = this.parseExpr(); this.expect(')'); return e; }
    if (this.peek() === '[') return this.parseArray();

    const ident = this.parseIdent();
    if (ident === 'true') return { type: 'Bool', value: true };
    if (ident === 'false') return { type: 'Bool', value: false };
    if (ident === 'null') return { type: 'Null' };

    this.skip();
    if (this.peek() === '(') {
      this.pos++;
      const args: Expr[] = [];
      this.skip();
      if (this.peek() !== ')') {
        args.push(this.parseExpr());
        while (this.match(',')) args.push(this.parseExpr());
      }
      this.expect(')');
      return { type: 'Call', name: ident, args };
    }

    let obj: Expr = { type: 'Var', name: ident };
    while (this.at('.')) {
      this.pos++;
      const prop = this.parseIdent();
      obj = { type: 'Member', object: obj, property: prop };
    }
    return obj;
  }

  private parseArray(): Expr {
    this.expect('[');
    const elems: Expr[] = [];
    this.skip();
    if (this.peek() !== ']') {
      elems.push(this.parseExpr());
      while (this.match(',')) elems.push(this.parseExpr());
    }
    this.expect(']');
    return { type: 'Array', elements: elems };
  }

  private parseBlock(): Stmt[] {
    this.expect('{');
    const stmts: Stmt[] = [];
    while (!this.at('}')) {
      stmts.push(this.parseStmt());
    }
    this.expect('}');
    return stmts;
  }

  parseStmt(): Stmt {
    this.skip();
    if (this.matchKeyword('var')) return this.parseVarDecl();
    if (this.matchKeyword('if')) return this.parseIf();
    if (this.matchKeyword('log')) return this.parseLog();
    if (this.matchKeyword('foreach')) return this.parseForeach();
    if (this.matchKeyword('on')) return this.parseOn();
    if (this.matchKeyword('return')) return this.parseReturn();
    return this.parseExprStmt();
  }

  private parseVarDecl(): Stmt {
    const name = this.parseIdent();
    let value: Expr = { type: 'Null' };
    if (this.match('=')) value = this.parseExpr();
    return { type: 'VarDecl', name, value };
  }

  private parseIf(): Stmt {
    this.expect('(');
    const condition = this.parseExpr();
    this.expect(')');
    const thenBody = this.parseBlock();
    let elseBody: Stmt[] | null = null;
    if (this.matchKeyword('else')) elseBody = this.parseBlock();
    return { type: 'If', condition, thenBody, elseBody };
  }

  private parseLog(): Stmt {
    this.expect('(');
    const expr = this.parseExpr();
    this.expect(')');
    return { type: 'Log', expr };
  }

  private parseForeach(): Stmt {
    this.expect('(');
    const itemVar = this.parseIdent();
    this.expect('in');
    const iterable = this.parseExpr();
    this.expect(')');
    const body = this.parseBlock();
    return { type: 'Foreach', itemVar, iterable, body };
  }

  private parseOn(): Stmt {
    const event = this.parseIdent();
    return { type: 'On', event };
  }

  private parseReturn(): Stmt {
    let value: Expr = { type: 'Null' };
    this.skip();
    if (this.peek() !== '}' && !this.eof()) value = this.parseExpr();
    return { type: 'Return', value };
  }

  private parseExprStmt(): Stmt {
    const expr = this.parseExpr();
    return { type: 'Expr', expr };
  }

  parseFnDef(): FnDef {
    const name = this.parseIdent();
    this.expect('(');
    const params: string[] = [];
    this.skip();
    if (this.peek() !== ')') {
      params.push(this.parseIdent());
      while (this.match(',')) params.push(this.parseIdent());
    }
    this.expect(')');
    const body = this.parseBlock();
    return { type: 'FnDef', name, params, body };
  }

  private parseDestructureParams(): string[] {
    this.expect('(');
    this.expect('{');
    const params: string[] = [];
    this.skip();
    if (this.peek() !== '}') {
      params.push(this.parseIdent());
      while (this.match(',')) params.push(this.parseIdent());
    }
    this.expect('}');
    this.expect(')');
    return params;
  }

  private parseWorkflowDef(): WorkflowDef {
    const name = this.parseString();
    this.skip();
    let params: string[] = [];
    if (this.peek() === '(') params = this.parseDestructureParams();
    const body = this.parseBlock();
    return { type: 'WorkflowDef', name, params, body };
  }

  parseProgram(): FlowProgram {
    const program: FlowProgram = { functions: [], workflows: [], stmts: [] };
    while (!this.eof()) {
      this.skip();
      if (this.eof()) break;
      if (this.matchKeyword('fn')) {
        program.functions.push(this.parseFnDef());
      } else if (this.matchKeyword('workflow')) {
        program.workflows.push(this.parseWorkflowDef());
      } else {
        program.stmts.push(this.parseStmt());
      }
    }
    return program;
  }
}

import type { FlowProgram, FnDef, WorkflowDef, Stmt, Expr } from './types.ts';

type Vars = Record<string, unknown>;

export class FlowEvaluator {
  private globals: Record<string, FnDef> = {};
  logs: string[] = [];
  events: string[] = [];

  evalExpr(expr: Expr, vars: Vars): unknown {
    switch (expr.type) {
      case 'String': return expr.value;
      case 'Number': return expr.value;
      case 'Bool': return expr.value;
      case 'Null': return null;
      case 'Var': return vars[expr.name] ?? this.globals[expr.name] ?? null;
      case 'Member': {
        const obj = this.evalExpr(expr.object, vars);
        if (obj == null) return null;
        if (expr.property === 'length') {
          if (Array.isArray(obj)) return obj.length;
          if (typeof obj === 'string') return obj.length;
          if (typeof obj === 'object') return Object.keys(obj as Record<string, unknown>).length;
          return 0;
        }
        if (typeof obj === 'object') return (obj as Record<string, unknown>)[expr.property] ?? null;
        return null;
      }
      case 'BinaryOp': {
        const l = this.evalExpr(expr.left, vars);
        const r = this.evalExpr(expr.right, vars);
        return this.evalBinary(expr.op, l, r);
      }
      case 'UnaryOp': {
        const val = this.evalExpr(expr.operand, vars);
        if (expr.op === '!') return !val;
        if (expr.op === '-') return -(Number(val) || 0);
        return null;
      }
      case 'Call': {
        const args = expr.args.map(a => this.evalExpr(a, vars));
        return this.callFn(expr.name, args);
      }
      case 'Array': return expr.elements.map(e => this.evalExpr(e, vars));
      default: return null;
    }
  }

  private evalBinary(op: string, l: unknown, r: unknown): unknown {
    switch (op) {
      case '+': return typeof l === 'string' || typeof r === 'string' ? String(l ?? '') + String(r ?? '') : (Number(l) || 0) + (Number(r) || 0);
      case '-': return (Number(l) || 0) - (Number(r) || 0);
      case '*': return (Number(l) || 0) * (Number(r) || 0);
      case '/': return (Number(r) || 0) !== 0 ? (Number(l) || 0) / Number(r) : null;
      case '%': return (Number(l) || 0) % (Number(r) || 0);
      case '==': return l === r;
      case '!=': return l !== r;
      case '<': return (l as number) < (r as number);
      case '>': return (l as number) > (r as number);
      case '<=': return (l as number) <= (r as number);
      case '>=': return (l as number) >= (r as number);
      case '&&': return !!l && !!r;
      case '||': return !!l || !!r;
      default: return null;
    }
  }

  private callFn(name: string, args: unknown[]): unknown {
    switch (name) {
      case 'log': this.logs.push(String(args[0] ?? '')); return null;
      case 'len': {
        const v = args[0];
        if (Array.isArray(v)) return v.length;
        if (typeof v === 'string') return v.length;
        return 0;
      }
      case 'to_string': return String(args[0] ?? '');
      case 'to_number': return Number(args[0]) || 0;
      default: return null;
    }
  }

  execStmt(stmt: Stmt, vars: Vars): void {
    switch (stmt.type) {
      case 'VarDecl':
        vars[stmt.name] = this.evalExpr(stmt.value, vars);
        break;
      case 'If': {
        const cond = this.evalExpr(stmt.condition, vars);
        if (cond) {
          for (const s of stmt.thenBody) this.execStmt(s, vars);
        } else if (stmt.elseBody) {
          for (const s of stmt.elseBody) this.execStmt(s, vars);
        }
        break;
      }
      case 'Log': {
        const val = this.evalExpr(stmt.expr, vars);
        this.logs.push(String(val ?? ''));
        break;
      }
      case 'Foreach': {
        const arr = this.evalExpr(stmt.iterable, vars);
        if (Array.isArray(arr)) {
          for (const item of arr) {
            vars[stmt.itemVar] = item;
            for (const s of stmt.body) this.execStmt(s, vars);
          }
        }
        break;
      }
      case 'On': break;
      case 'Return': break;
      case 'Expr': this.evalExpr(stmt.expr, vars); break;
      default: break;
    }
  }

  executeWorkflow(workflow: WorkflowDef, eventData: Record<string, unknown>): string[] {
    this.logs = [];
    const vars: Vars = { data: eventData };

    if (workflow.params && workflow.params.length > 0) {
      for (const p of workflow.params) {
        vars[p] = (eventData as Record<string, unknown>)[p] ?? null;
      }
    }

    for (const stmt of workflow.body) {
      this.execStmt(stmt, vars);
    }

    return this.logs;
  }

  loadProgram(program: FlowProgram): void {
    for (const fn of program.functions) {
      this.globals[fn.name] = fn;
    }
  }
}

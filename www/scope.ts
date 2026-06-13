
import type { FlowProgram, Stmt, Expr, FnDef, WorkflowDef } from '@src/types.ts';
import type { SchemaType } from './schema.ts';
import { getSchemaForPath, getSchemaType } from './schema.ts';

export interface VarInfo {
  type: string;
  schema?: SchemaType;
}

export interface ScopeInfo {
  variables: Map<string, VarInfo>;
  schema: SchemaType;
}

export function buildScope(
  program: FlowProgram,
  schema: SchemaType,
  cursorPos: number,
  code: string
): ScopeInfo {
  const variables = new Map<string, VarInfo>();
  const upToCursor = code.substring(0, cursorPos);

  for (const fn of program.functions) {
    for (const param of fn.params) {
      variables.set(param, { type: 'any' });
    }
    inferTypesFromStmts(fn.body, variables, schema, upToCursor);
  }

  for (const wf of program.workflows) {
    for (const param of wf.params) {
      variables.set(param, { type: 'any' });
    }
    inferTypesFromStmts(wf.body, variables, schema, upToCursor);
  }

  return { variables, schema };
}

function inferTypesFromStmts(
  stmts: Stmt[],
  variables: Map<string, VarInfo>,
  schema: SchemaType,




  upToCursor: string
): void {
  for (const stmt of stmts) {
    if (stmt.type === 'VarDecl') {
      const inferred = inferTypeFromExpr(stmt.value, variables, schema);
      variables.set(stmt.name, inferred);
    } else if (stmt.type === 'Foreach') {
      const iterableType = inferTypeFromExpr(stmt.iterable, variables, schema);
      const itemType = getIterableItemType(iterableType, stmt.iterable, variables, schema);
      variables.set(stmt.itemVar, itemType);
      inferTypesFromStmts(stmt.body, variables, schema, upToCursor);
    } else if (stmt.type === 'If') {
      inferTypesFromStmts(stmt.thenBody, variables, schema, upToCursor);
      if (stmt.elseBody) {
        inferTypesFromStmts(stmt.elseBody, variables, schema, upToCursor);
      }
    } else if (stmt.type === 'Expr' && stmt.expr.type === 'Call') {
      for (const arg of stmt.expr.args) {
        inferTypeFromExpr(arg, variables, schema);
      }
    }
  }
}

function inferTypeFromExpr(
  expr: Expr,
  variables: Map<string, VarInfo>,
  schema: SchemaType
): VarInfo {
  switch (expr.type) {
    case 'String':
      return { type: 'string' };
    case 'Number':
      return { type: 'number' };
    case 'Bool':
      return { type: 'boolean' };
    case 'Null':
      return { type: 'null' };
    case 'Var':
      return variables.get(expr.name) || { type: 'any' };
    case 'Member':
      return inferMemberType(expr, variables, schema);
    case 'BinaryOp':
      return inferBinaryType(expr, variables, schema);
    case 'Call':
      return inferCallType(expr);
    case 'Array':
      if (expr.elements.length > 0) {
        return inferTypeFromExpr(expr.elements[0], variables, schema);
      }
      return { type: 'array' };
    default:
      return { type: 'any' };
  }
}

function inferMemberType(
  expr: { type: 'Member'; object: Expr; property: string },
  variables: Map<string, VarInfo>,
  schema: SchemaType
): VarInfo {
  const objType = inferTypeFromExpr(expr.object, variables, schema);

  if (objType.schema && typeof objType.schema === 'object') {
    const propValue = objType.schema[expr.property];
    if (propValue !== undefined) {
      return {
        type: getSchemaType(propValue),
        schema: typeof propValue === 'object' && !Array.isArray(propValue) ? propValue as SchemaType : undefined,
      };
    }
  }

  if (expr.object.type === 'Var' && expr.object.name === 'data') {
    const propValue = schema[expr.property];
    if (propValue !== undefined) {
      return {
        type: getSchemaType(propValue),
        schema: typeof propValue === 'object' && !Array.isArray(propValue) ? propValue as SchemaType : undefined,
      };
    }
  }

  if (expr.object.type === 'Member' && expr.object.object.type === 'Var' && expr.object.object.name === 'data') {
    const path = `${expr.object.property}.${expr.property}`;
    const subSchema = getSchemaForPath(schema, path);
    if (subSchema) {
      return { type: getSchemaType(subSchema), schema: subSchema };
    }
  }

  return { type: 'any' };
}

function inferBinaryType(
  expr: { type: 'BinaryOp'; op: string; left: Expr; right: Expr },
  variables: Map<string, VarInfo>,
  schema: SchemaType
): VarInfo {
  if (expr.op === '+' || expr.op === '-' || expr.op === '*' || expr.op === '/') {
    const left = inferTypeFromExpr(expr.left, variables, schema);
    const right = inferTypeFromExpr(expr.right, variables, schema);

    if (left.type === 'string' || right.type === 'string') {
      return { type: 'string' };
    }
    if (left.type === 'number' && right.type === 'number') {
      return { type: 'number' };
    }
  }

  if (expr.op === '==' || expr.op === '!=' || expr.op === '<' || expr.op === '>' ||
      expr.op === '<=' || expr.op === '>=' || expr.op === '&&' || expr.op === '||') {
    return { type: 'boolean' };
  }

  return { type: 'any' };
}

function inferCallType(expr: { type: 'Call'; name: string; args: Expr[] }): VarInfo {
  switch (expr.name) {
    case 'len':
      return { type: 'number' };
    case 'to_string':
      return { type: 'string' };
    case 'to_number':
      return { type: 'number' };
    case 'log':
      return { type: 'void' };
    default:
      return { type: 'any' };
  }
}

function getIterableItemType(
  iterableType: VarInfo,
  iterable: Expr,
  variables: Map<string, VarInfo>,
  schema: SchemaType
): VarInfo {
  if (iterableType.schema && typeof iterableType.schema === 'object') {
    const keys = Object.keys(iterableType.schema);
    if (keys.length > 0) {
      const firstKey = keys[0];
      const firstValue = iterableType.schema[firstKey];
      return {
        type: typeof firstValue === 'string' ? firstValue : 'object',
        schema: typeof firstValue === 'object' && !Array.isArray(firstValue) ? firstValue as SchemaType : undefined,
      };
    }
  }

  if (iterable.type === 'Var' && iterable.name === 'data') {
    const topKeys = Object.keys(schema);
    if (topKeys.length > 0) {
      const firstValue = schema[topKeys[0]];
      if (Array.isArray(firstValue) && firstValue.length > 0 && typeof firstValue[0] === 'object') {
        return {
          type: 'object',
          schema: firstValue[0] as SchemaType,
        };
      }
    }
  }

  if (iterable.type === 'Member' && iterable.object.type === 'Var' && iterable.object.name === 'data') {
    const propSchema = schema[iterable.property];
    if (Array.isArray(propSchema) && propSchema.length > 0 && typeof propSchema[0] === 'object') {
      return {
        type: 'object',
        schema: propSchema[0] as SchemaType,
      };
    }
  }

  return { type: 'any' };
}

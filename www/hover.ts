import type { FlowProgram } from '@src/types.ts';
import type { SchemaType } from './schema.ts';
import { getSchemaType, getSchemaForPath } from './schema.ts';

export interface HoverInfo {
  content: string;
  range: { start: number; end: number };
}

const BUILTIN_FUNCS: Record<string, string> = {
  'log': 'log(msg: string): void\nLog a message to output',
  'len': 'len(arr: array): number\nGet length of array',
  'to_string': 'to_string(val: any): string\nConvert value to string',
  'to_number': 'to_number(val: any): number\nConvert value to number',
};

export function getHoverInfo(
  code: string,
  cursorPos: number,
  schema: SchemaType,
  program: FlowProgram | null
): HoverInfo | null {
  const wordRange = getWordAtPos(code, cursorPos);
  if (!wordRange) return null;

  const word = code.substring(wordRange.start, wordRange.end);

  const dotExpr = getDotExpressionAtPos(code, cursorPos);
  if (dotExpr) {
    const hoverInfo = getSchemaHoverInfo(dotExpr, schema);
    if (hoverInfo) {
      return {
        content: hoverInfo,
        range: { start: dotExpr.start, end: dotExpr.end },
      };
    }
  }

  if (BUILTIN_FUNCS[word]) {
    return {
      content: BUILTIN_FUNCS[word],
      range: wordRange,
    };
  }

  if (program) {
    const fnHover = getFunctionHoverInfo(word, program);
    if (fnHover) {
      return {
        content: fnHover,
        range: wordRange,
      };
    }

    const varHover = getVariableHoverInfo(word, code, cursorPos, schema);
    if (varHover) {
      return {
        content: varHover,
        range: wordRange,
      };
    }
  }

  const keywordHover = getKeywordHoverInfo(word);
  if (keywordHover) {
    return {
      content: keywordHover,
      range: wordRange,
    };
  }

  return null;
}

function getWordAtPos(code: string, pos: number): { start: number; end: number } | null {
  if (pos < 0 || pos >= code.length) return null;
  if (!/[a-zA-Z_]/.test(code[pos])) return null;

  let start = pos;
  while (start > 0 && /[a-zA-Z0-9_]/.test(code[start - 1])) start--;

  let end = pos;
  while (end < code.length && /[a-zA-Z0-9_]/.test(code[end])) end++;

  return { start, end };
}

interface DotExpr {
  full: string;
  start: number;
  end: number;
}

function getDotExpressionAtPos(code: string, pos: number): DotExpr | null {
  let end = pos;
  while (end < code.length && /[a-zA-Z0-9_]/.test(code[end])) end++;

  let start = pos;
  while (start > 0 && /[a-zA-Z0-9_.]/.test(code[start - 1])) start--;

  const expr = code.substring(start, end);
  if (!expr.includes('.')) return null;

  if (!/^[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)+$/.test(expr)) return null;

  return { full: expr, start, end };
}

function getSchemaHoverInfo(dotExpr: DotExpr, schema: SchemaType): string | null {
  const parts = dotExpr.full.split('.');
  if (parts[0] !== 'data') return null;

  const dataPath = parts.slice(1).join('.');
  const subSchema = getSchemaForPath(schema, dataPath);

  if (subSchema && typeof subSchema === 'object') {
    const typeStr = getSchemaType(subSchema);
    const keys = Object.keys(subSchema);

    if (keys.length <= 5) {
      const props = keys.map(k => {
        const val = subSchema[k];
        const t = getSchemaType(val);
        return `  ${k}: ${t}`;
      }).join('\n');
      return `data.${dataPath}: ${typeStr}\n{\n${props}\n}`;
    }
    return `data.${dataPath}: ${typeStr}\n${keys.length} properties`;
  }

  const topLevel = schema[parts[1]];
  if (topLevel) {
    const typeStr = getSchemaType(topLevel);
    return `data.${parts[1]}: ${typeStr}`;
  }

  return null;
}

function getFunctionHoverInfo(name: string, program: FlowProgram): string | null {
  for (const fn of program.functions) {
    if (fn.name === name) {
      const params = fn.params.join(', ');
      return `fn ${name}(${params})\nUser-defined function`;
    }
  }
  return null;
}

function getVariableHoverInfo(
  name: string,
  code: string,
  cursorPos: number,
  schema: SchemaType
): string | null {
  const upToCursor = code.substring(0, cursorPos);

  const varRegex = new RegExp(`var\\s+${escapeRegex(name)}\\s*=\\s*(.+)`, 'g');
  const match = varRegex.exec(upToCursor);
  if (match) {
    const initExpr = match[1].trim();
    const inferredType = inferType(initExpr, schema);
    return `var ${name}: ${inferredType}`;
  }

  const foreachRegex = new RegExp(`foreach\\s*\\(\\s*${escapeRegex(name)}\\s+in\\s+(\\w+)`, 'g');
  const foreachMatch = foreachRegex.exec(upToCursor);
  if (foreachMatch) {
    const iterable = foreachMatch[1];
    if (iterable === 'data') {
      const topKeys = Object.keys(schema);
      return `var ${name}: any (iterating data)`;
    }
    return `var ${name}: any`;
  }

  return null;
}

function inferType(expr: string, schema: SchemaType): string {
  if (/^".*"$/.test(expr)) return 'string';
  if (/^-?\d+(\.\d+)?$/.test(expr)) return 'number';
  if (/^(true|false)$/.test(expr)) return 'boolean';
  if (/^null$/.test(expr)) return 'null';
  if (/^\[.*\]$/.test(expr)) return 'array';

  if (/^\w+\(.*\)$/.test(expr)) {
    const fnMatch = expr.match(/^(\w+)\(/);
    if (fnMatch) {
      const fnName = fnMatch[1];
      if (fnName === 'len') return 'number';
      if (fnName === 'to_string') return 'string';
      if (fnName === 'to_number') return 'number';
    }
    return 'any';
  }

  if (/^data\./.test(expr)) {
    const dataPath = expr.replace(/^data\./, '');
    const subSchema = getSchemaForPath(schema, dataPath);
    if (subSchema) return getSchemaType(subSchema);
  }

  return 'any';
}

function getKeywordHoverInfo(word: string): string | null {
  const keywords: Record<string, string> = {
    'workflow': 'workflow "name" { ... }\nDefine a workflow triggered by an event',
    'fn': 'fn name(params) { ... }\nDefine a reusable function',
    'var': 'var name = value\nDeclare a variable',
    'if': 'if (condition) { ... }\nConditional execution',
    'else': 'else { ... }\nAlternative branch',
    'foreach': 'foreach (item in collection) { ... }\nIterate over array',
    'in': 'Used in foreach: foreach (item in collection)',
    'on': 'on EVENT_NAME\nTrigger workflow on event',
    'return': 'return value\nReturn from function',
    'true': 'Boolean literal: true',
    'false': 'Boolean literal: false',
    'null': 'Null literal',
    'import': 'import name from "path"\nImport schema from file',
    'from': 'Used in import: import name from "path"',
  };

  return keywords[word] || null;
}

function escapeRegex(str: string): string {
  return str.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

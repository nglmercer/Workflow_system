import type { FlowProgram } from '@src/types.ts';
import type { SchemaType } from './schema.ts';
import { getSchemaType, getSchemaForPath } from './schema.ts';
import { buildScope, type ScopeInfo } from './scope.ts';

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
  const scope = program ? buildScope(program, schema, cursorPos, code) : null;

  const wordRange = getWordAtPos(code, cursorPos);
  if (!wordRange) return null;

  const word = code.substring(wordRange.start, wordRange.end);

  const dotExpr = getDotExpressionAtPos(code, cursorPos);
  if (dotExpr) {
    const hoverInfo = getDotExpressionHoverInfo(dotExpr, schema, scope);
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
  }

  if (scope) {
    const varInfo = scope.variables.get(word);
    if (varInfo && varInfo.type !== 'any') {
      let content = `var ${word}: ${varInfo.type}`;
      if (varInfo.schema) {
        const keys = Object.keys(varInfo.schema);
        if (keys.length > 0 && keys.length <= 5) {
          const props = keys.map(k => {
            const val = varInfo.schema![k];
            const t = getSchemaType(val);
            return `  ${k}: ${t}`;
          }).join('\n');
          content += `\n{\n${props}\n}`;
        }
      }
      return {
        content,
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

function getDotExpressionHoverInfo(
  dotExpr: DotExpr,
  schema: SchemaType,
  scope: ScopeInfo | null
): string | null {
  const parts = dotExpr.full.split('.');

  if (parts[0] === 'data') {
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
  }

  if (scope) {
    const varName = parts[0];
    const varInfo = scope.variables.get(varName);
    if (varInfo && varInfo.schema) {
      let currentSchema: SchemaType | undefined = varInfo.schema;
      for (let i = 1; i < parts.length && currentSchema; i++) {
        const propValue = currentSchema[parts[i]];
        if (propValue === undefined) return null;

        if (i === parts.length - 1) {
          const typeStr = getSchemaType(propValue);
          if (typeof propValue === 'object' && !Array.isArray(propValue) && propValue !== null) {
            const keys = Object.keys(propValue);
            if (keys.length <= 5) {
              const props = keys.map(k => {
                const val = propValue[k];
                const t = getSchemaType(val);
                return `  ${k}: ${t}`;
              }).join('\n');
              return `${dotExpr.full}: ${typeStr}\n{\n${props}\n}`;
            }
            return `${dotExpr.full}: ${typeStr}\n${keys.length} properties`;
          }
          return `${dotExpr.full}: ${typeStr}`;
        }

        if (typeof propValue === 'object' && !Array.isArray(propValue) && propValue !== null) {
          currentSchema = propValue as SchemaType;
        } else if (Array.isArray(propValue) && propValue.length > 0 && typeof propValue[0] === 'object') {
          currentSchema = propValue[0] as SchemaType;
        } else {
          return null;
        }
      }
    }
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

  for (const wf of program.workflows) {
    if (wf.name === name) {
      const params = wf.params.length > 0 ? `({${wf.params.join(', ')}})` : '';
      return `workflow "${wf.name}"\non ${wf.event}${params}`;
    }
  }

  return null;
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
    'emit': 'emit("WORKFLOW_NAME", data)\nTrigger another workflow with data',
    'return': 'return value\nReturn from function',
    'true': 'Boolean literal: true',
    'false': 'Boolean literal: false',
    'null': 'Null literal',
    'import': 'import name from "path"\nImport schema from file',
    'from': 'Used in import: import name from "path"',
  };

  return keywords[word] || null;
}

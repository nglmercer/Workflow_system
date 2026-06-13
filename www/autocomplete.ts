import type { FlowProgram } from '@src/types.ts';
import type { SchemaType } from './schema.ts';
import { getSchemaType, getSchemaForPath } from './schema.ts';
import { buildScope, type ScopeInfo } from './scope.ts';

export interface CompletionItem {
  label: string;
  kind: 'keyword' | 'function' | 'variable' | 'property' | 'snippet' | 'workflow';
  detail?: string;
  insertText: string;
  arguments?: ArgInfo[];
}

export interface ArgInfo {
  name: string;
  type: string;
  defaultValue?: string;
}

const KEYWORDS = [
  'workflow', 'fn', 'var', 'if', 'else', 'foreach', 'in', 'on', 'return',
  'true', 'false', 'null', 'import', 'from', 'emit',
];

const BUILTIN_FUNCS: Record<string, { signature: string; args: ArgInfo[] }> = {
  'log': {
    signature: 'log(msg)',
    args: [{ name: 'msg', type: 'string', defaultValue: '""' }],
  },
  'len': {
    signature: 'len(arr)',
    args: [{ name: 'arr', type: 'array | string', defaultValue: '[]' }],
  },
  'to_string': {
    signature: 'to_string(val)',
    args: [{ name: 'val', type: 'any', defaultValue: '' }],
  },
  'to_number': {
    signature: 'to_number(val)',
    args: [{ name: 'val', type: 'string | number', defaultValue: '0' }],
  },
};

function isInsideString(code: string, pos: number): boolean {
  let inString = false;
  let stringChar = '';
  let escaped = false;

  for (let i = 0; i < pos; i++) {
    const ch = code[i];

    if (escaped) {
      escaped = false;
      continue;
    }

    if (ch === '\\') {
      escaped = true;
      continue;
    }

    if (ch === '"' || ch === "'") {
      if (inString && ch === stringChar) {
        inString = false;
        stringChar = '';
      } else if (!inString) {
        inString = true;
        stringChar = ch;
      }
    }
  }

  return inString;
}

function isInsideComment(code: string, pos: number): boolean {
  for (let i = 0; i < pos - 1; i++) {
    if (code[i] === '/' && code[i + 1] === '/') {
      let lineEnd = code.indexOf('\n', i);
      if (lineEnd === -1) lineEnd = code.length;
      if (pos <= lineEnd) return true;
      i = lineEnd;
    }
  }
  return false;
}

export function getCompletions(
  code: string,
  cursorPos: number,
  schema: SchemaType,
  program: FlowProgram | null
): CompletionItem[] {
  if (isInsideString(code, cursorPos) || isInsideComment(code, cursorPos)) {
    return [];
  }

  const scope = program ? buildScope(program, schema, cursorPos, code) : null;
  const before = code.substring(0, cursorPos);
  const items: CompletionItem[] = [];

  const funcCallMatch = before.match(/(\w+)\(([^)]*)$/);
  if (funcCallMatch) {
    const funcName = funcCallMatch[1];
    const argsSoFar = funcCallMatch[2];

    const funcInfo = getFunctionInfo(funcName, program);
    if (funcInfo) {
      return [{
        label: funcInfo.signature,
        kind: 'snippet',
        detail: funcInfo.detail,
        insertText: buildSnippet(funcInfo.args, argsSoFar),
        arguments: funcInfo.args,
      }];
    }
  }

  const emitMatch = before.match(/emit\s*\(\s*"(\w*)"$/);
  if (emitMatch) {
    if (program) {
      for (const wf of program.workflows) {
        items.push({
          label: wf.name,
          kind: 'workflow',
          detail: `on ${wf.event}`,
          insertText: wf.name,
        });
      }
    }
    return items;
  }

  const emitPartialMatch = before.match(/emit\s*\(\s*"/);
  if (emitPartialMatch) {
    if (program) {
      for (const wf of program.workflows) {
        items.push({
          label: wf.name,
          kind: 'workflow',
          detail: `on ${wf.event}`,
          insertText: wf.name,
        });
      }
    }
    return items;
  }

  const dotMatch = before.match(/(\w+(?:\.\w+)*)\.(\w*)$/);
  if (dotMatch) {
    const path = dotMatch[1];
    const prefix = dotMatch[2];
    const parts = path.split('.');

    if (parts[0] === 'data' && Object.keys(schema).length > 0) {
      const dataPath = parts.slice(1).join('.');
      const subSchema = dataPath ? getSchemaForPath(schema, dataPath) : schema;

      if (subSchema && typeof subSchema === 'object') {
        for (const [key, value] of Object.entries(subSchema)) {
          if (prefix && !key.toLowerCase().startsWith(prefix.toLowerCase())) continue;
          const typeStr = getSchemaType(value);
          items.push({
            label: key,
            kind: 'property',
            detail: typeStr,
            insertText: key,
          });
        }
        return items;
      }
    }

    if (scope) {
      const varName = parts[0];
      const varInfo = scope.variables.get(varName);
      if (varInfo && varInfo.schema) {
        let currentSchema: SchemaType | undefined = varInfo.schema;
        for (let i = 1; i < parts.length && currentSchema; i++) {
          const propValue = currentSchema[parts[i]];
          if (propValue === undefined) return items;

          if (i === parts.length - 1) {
            if (typeof propValue === 'object' && !Array.isArray(propValue) && propValue !== null) {
              currentSchema = propValue as SchemaType;
            } else {
              return items;
            }
          } else if (typeof propValue === 'object' && !Array.isArray(propValue) && propValue !== null) {
            currentSchema = propValue as SchemaType;
          } else if (Array.isArray(propValue) && propValue.length > 0 && typeof propValue[0] === 'object') {
            currentSchema = propValue[0] as SchemaType;
          } else {
            return items;
          }
        }

        if (currentSchema) {
          for (const [key, value] of Object.entries(currentSchema)) {
            if (prefix && !key.toLowerCase().startsWith(prefix.toLowerCase())) continue;
            const typeStr = getSchemaType(value);
            items.push({
              label: key,
              kind: 'property',
              detail: typeStr,
              insertText: key,
            });
          }
          return items;
        }
      }
    }

    return items;
  }

  const wordMatch = before.match(/(\w+)$/);
  if (!wordMatch) return items;
  const word = wordMatch[1];
  const wordLower = word.toLowerCase();

  for (const kw of KEYWORDS) {
    const kwLower = kw.toLowerCase();
    if (kwLower.startsWith(wordLower) && kwLower !== wordLower) {
      items.push({
        label: kw,
        kind: 'keyword',
        insertText: kw,
      });
    }
  }

  for (const [name, info] of Object.entries(BUILTIN_FUNCS)) {
    const nameLower = name.toLowerCase();
    if (nameLower.startsWith(wordLower) && nameLower !== wordLower) {
      items.push({
        label: name,
        kind: 'function',
        detail: info.signature,
        insertText: name,
        arguments: info.args,
      });
    }
  }

  if (program) {
    for (const fn of program.functions) {
      const fnLower = fn.name.toLowerCase();
      if (fnLower.startsWith(wordLower) && fnLower !== wordLower) {
        const args: ArgInfo[] = fn.params.map(p => ({
          name: p,
          type: 'any',
          defaultValue: '',
        }));
        const params = fn.params.join(', ');
        items.push({
          label: fn.name,
          kind: 'function',
          detail: `fn ${fn.name}(${params})`,
          insertText: fn.name,
          arguments: args,
        });
      }
    }

    for (const wf of program.workflows) {
      const wfLower = wf.name.toLowerCase();
      if (wfLower.startsWith(wordLower) && wfLower !== wordLower) {
        items.push({
          label: wf.name,
          kind: 'workflow',
          detail: `workflow on ${wf.event}`,
          insertText: wf.name,
        });
      }
    }
  }

  if (scope) {
    for (const [name, info] of scope.variables) {
      const nameLower = name.toLowerCase();
      if (nameLower.startsWith(wordLower) && nameLower !== wordLower && !items.some(i => i.label === name)) {
        items.push({
          label: name,
          kind: 'variable',
          detail: info.type,
          insertText: name,
        });
      }
    }
  }

  return items;
}

interface FuncInfo {
  name: string;
  signature: string;
  detail: string;
  args: ArgInfo[];
}

function getFunctionInfo(name: string, program: FlowProgram | null): FuncInfo | null {
  if (BUILTIN_FUNCS[name]) {
    const info = BUILTIN_FUNCS[name];
    return {
      name,
      signature: info.signature,
      detail: info.signature,
      args: info.args,
    };
  }

  if (program) {
    for (const fn of program.functions) {
      if (fn.name === name) {
        const args: ArgInfo[] = fn.params.map(p => ({
          name: p,
          type: 'any',
          defaultValue: '',
        }));
        return {
          name,
          signature: `fn ${fn.name}(${fn.params.join(', ')})`,
          detail: `fn ${fn.name}(${fn.params.join(', ')})`,
          args,
        };
      }
    }
  }

  return null;
}

function buildSnippet(args: ArgInfo[], argsSoFar: string): string {
  if (args.length === 0) return '';

  const existingArgs = argsSoFar.split(',').map(a => a.trim()).filter(Boolean);
  const remaining = args.slice(existingArgs.length);

  if (remaining.length === 0) return '';

  const snippetParts: string[] = [];
  for (let i = 0; i < remaining.length; i++) {
    const arg = remaining[i];
    const placeholder = arg.defaultValue || arg.name;
    snippetParts.push(`\${${i + 1}:${placeholder}}`);
  }

  const needsCommaBefore = existingArgs.length > 0 && remaining.length > 0;
  return (needsCommaBefore ? ', ' : '') + snippetParts.join(', ');
}

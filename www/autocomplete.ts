import type { FlowProgram } from '@src/types.ts';
import type { SchemaType } from './schema.ts';
import { getSchemaType, getSchemaForPath } from './schema.ts';

export interface CompletionItem {
  label: string;
  kind: 'keyword' | 'function' | 'variable' | 'property' | 'snippet';
  detail?: string;
  insertText: string;
}

const KEYWORDS = [
  'workflow', 'fn', 'var', 'if', 'else', 'foreach', 'in', 'on', 'return',
  'true', 'false', 'null', 'import', 'from',
];

const BUILTIN_FUNCS: Record<string, string> = {
  'log': 'log(msg)',
  'len': 'len(arr)',
  'to_string': 'to_string(val)',
  'to_number': 'to_number(val)',
};

export function getCompletions(
  code: string,
  cursorPos: number,
  schema: SchemaType,
  program: FlowProgram | null
): CompletionItem[] {
  const before = code.substring(0, cursorPos);
  const items: CompletionItem[] = [];

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

    if (parts[0] === 'data') {
      return items;
    }
  }

  const wordMatch = before.match(/(\w+)$/);
  if (!wordMatch) return items;
  const word = wordMatch[1];
  const wordLower = word.toLowerCase();

  for (const kw of KEYWORDS) {
    if (kw.toLowerCase().startsWith(wordLower)) {
      items.push({
        label: kw,
        kind: 'keyword',
        insertText: kw,
      });
    }
  }

  for (const [name, sig] of Object.entries(BUILTIN_FUNCS)) {
    if (name.toLowerCase().startsWith(wordLower)) {
      items.push({
        label: name,
        kind: 'function',
        detail: sig,
        insertText: name,
      });
    }
  }

  if (program) {
    for (const fn of program.functions) {
      if (fn.name.toLowerCase().startsWith(wordLower)) {
        const params = fn.params.join(', ');
        items.push({
          label: fn.name,
          kind: 'function',
          detail: `fn ${fn.name}(${params})`,
          insertText: fn.name,
        });
      }
    }

    const varNames = collectVariables(code, cursorPos);
    for (const v of varNames) {
      if (v.toLowerCase().startsWith(wordLower) && !items.some(i => i.label === v)) {
        items.push({
          label: v,
          kind: 'variable',
          insertText: v,
        });
      }
    }
  }

  return items;
}

function collectVariables(code: string, cursorPos: number): string[] {
  const vars = new Set<string>();
  const upToCursor = code.substring(0, cursorPos);

  const varDeclRegex = /var\s+(\w+)/g;
  let m;
  while ((m = varDeclRegex.exec(upToCursor)) !== null) {
    vars.add(m[1]);
  }

  const foreachRegex = /foreach\s*\(\s*(\w+)\s+in\s+/g;
  while ((m = foreachRegex.exec(upToCursor)) !== null) {
    vars.add(m[1]);
  }

  const fnParamRegex = /fn\s+\w+\s*\(([^)]*)\)/g;
  while ((m = fnParamRegex.exec(upToCursor)) !== null) {
    const params = m[1].split(',').map(p => p.trim()).filter(Boolean);
    for (const p of params) {
      vars.add(p);
    }
  }

  const onParamRegex = /on\s+\w+\s*\(\s*\{([^}]*)\}\s*\)/g;
  while ((m = onParamRegex.exec(upToCursor)) !== null) {
    const params = m[1].split(',').map(p => p.trim()).filter(Boolean);
    for (const p of params) {
      vars.add(p);
    }
  }

  return Array.from(vars);
}

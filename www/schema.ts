import type { FlowProgram } from '@src/types.ts';

export interface SchemaType {
  [key: string]: SchemaType | SchemaType[] | string;
}

export async function fetchSchema(path: string): Promise<SchemaType | null> {
  try {
    if (path.startsWith('http://') || path.startsWith('https://')) {
      const resp = await fetch(path);
      if (!resp.ok) return null;
      const data = await resp.json();
      return normalizeSchema(data);
    }

    if (path.startsWith('./') || path.startsWith('../') || path.startsWith('/')) {
      const resp = await fetch(path);
      if (!resp.ok) return null;
      const data = await resp.json();
      return normalizeSchema(data);
    }

    return null;
  } catch {
    return null;
  }
}

function normalizeSchema(obj: unknown): SchemaType {
  if (obj === null || obj === undefined) return {};
  if (typeof obj !== 'object') return {};

  const schema: SchemaType = {};
  const record = obj as Record<string, unknown>;

  for (const [key, value] of Object.entries(record)) {
    if (value === null || value === undefined) {
      schema[key] = 'any';
    } else if (typeof value === 'string') {
      schema[key] = value;
    } else if (typeof value === 'number') {
      schema[key] = 'number';
    } else if (typeof value === 'boolean') {
      schema[key] = 'boolean';
    } else if (Array.isArray(value)) {
      if (value.length > 0 && typeof value[0] === 'object') {
        schema[key] = [normalizeSchema(value[0])];
      } else {
        schema[key] = 'array';
      }
    } else if (typeof value === 'object') {
      schema[key] = normalizeSchema(value);
    }
  }

  return schema;
}

export function getSchemaType(value: SchemaType | SchemaType[] | string): string {
  if (typeof value === 'string') return value;
  if (Array.isArray(value)) {
    if (value.length > 0 && typeof value[0] === 'object') {
      const inner = getSchemaType(value[0]);
      return `Array<${inner}>`;
    }
    return 'array';
  }
  if (typeof value === 'object' && value !== null) {
    const keys = Object.keys(value);
    if (keys.length <= 3) {
      const props = keys.map(k => `${k}: ${getSchemaType(value[k])}`).join(', ');
      return `{${props}}`;
    }
    return `object (${keys.length} props)`;
  }
  return 'any';
}

export function getSchemaForPath(schema: SchemaType, path: string): SchemaType | null {
  const parts = path.split('.');
  let current: Record<string, unknown> = schema as Record<string, unknown>;

  for (const part of parts) {
    if (current === null || current === undefined) return null;
    if (typeof current !== 'object') return null;

    const value = current[part];
    if (value === null || value === undefined) return null;

    if (typeof value === 'string') {
      if (part === parts[parts.length - 1]) return null;
      current = {};
    } else if (Array.isArray(value)) {
      if (value.length > 0 && typeof value[0] === 'object' && value[0] !== null) {
        current = value[0] as Record<string, unknown>;
      } else {
        return null;
      }
    } else if (typeof value === 'object') {
      current = value as Record<string, unknown>;
    } else {
      return null;
    }
  }

  if (typeof current === 'object' && current !== null && !Array.isArray(current)) {
    return current as SchemaType;
  }
  return null;
}

export function getPropertiesAtCursor(
  code: string,
  cursorPos: number,
  schema: SchemaType
): string[] {
  const before = code.substring(0, cursorPos);
  const match = before.match(/(\w+(?:\.\w+)*)\.$/);

  if (!match) {
    if (before.endsWith('.')) {
      const varMatch = before.match(/(\w+)\.$/);
      if (varMatch && varMatch[1] === 'data') {
        return Object.keys(schema);
      }
    }
    return [];
  }

  const path = match[1];
  const parts = path.split('.');

  if (parts[0] !== 'data') return [];

  const dataPath = parts.slice(1).join('.');
  const subSchema = dataPath ? getSchemaForPath(schema, dataPath) : schema;

  if (!subSchema) return [];

  return Object.keys(subSchema);
}

export async function loadSchemasFromProgram(program: FlowProgram): Promise<SchemaType> {
  const merged: SchemaType = {};

  for (const imp of program.imports) {
    const schema = await fetchSchema(imp.path);
    if (schema) {
      Object.assign(merged, schema);
    }
  }

  return merged;
}

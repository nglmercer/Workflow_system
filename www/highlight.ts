export type TokenKind =
  | 'keyword'
  | 'string'
  | 'number'
  | 'comment'
  | 'function'
  | 'operator'
  | 'punctuation'
  | 'property'
  | 'variable'
  | 'text';

export interface Token {
  kind: TokenKind;
  value: string;
}

const KEYWORDS = new Set([
  'workflow', 'fn', 'var', 'if', 'else', 'foreach', 'in', 'on', 'return',
  'true', 'false', 'null', 'import', 'from',
]);

const BUILTIN_FUNCS = new Set([
  'log', 'len', 'to_string', 'to_number',
]);

export function tokenize(input: string): Token[] {
  const tokens: Token[] = [];
  let i = 0;

  while (i < input.length) {
    const ch = input[i];

    // Comments
    if (ch === '/' && input[i + 1] === '/') {
      let end = input.indexOf('\n', i);
      if (end === -1) end = input.length;
      tokens.push({ kind: 'comment', value: input.slice(i, end) });
      i = end;
      continue;
    }

    // Strings
    if (ch === '"') {
      let j = i + 1;
      while (j < input.length && input[j] !== '"') {
        if (input[j] === '\\') j++;
        j++;
      }
      tokens.push({ kind: 'string', value: input.slice(i, j + 1) });
      i = j + 1;
      continue;
    }

    // Numbers
    if (/[0-9]/.test(ch) || (ch === '-' && i + 1 < input.length && /[0-9]/.test(input[i + 1]))) {
      let j = i;
      if (input[j] === '-') j++;
      while (j < input.length && /[0-9]/.test(input[j])) j++;
      if (j < input.length && input[j] === '.') {
        j++;
        while (j < input.length && /[0-9]/.test(input[j])) j++;
      }
      tokens.push({ kind: 'number', value: input.slice(i, j) });
      i = j;
      continue;
    }

    // Identifiers and keywords
    if (/[a-zA-Z_]/.test(ch)) {
      let j = i;
      while (j < input.length && /[a-zA-Z0-9_]/.test(input[j])) j++;
      const word = input.slice(i, j);

      if (KEYWORDS.has(word)) {
        tokens.push({ kind: 'keyword', value: word });
      } else if (BUILTIN_FUNCS.has(word)) {
        tokens.push({ kind: 'function', value: word });
      } else if (j < input.length && input[j] === '(') {
        tokens.push({ kind: 'function', value: word });
      } else {
        tokens.push({ kind: 'variable', value: word });
      }
      i = j;
      continue;
    }

    // Multi-char operators
    if (i + 1 < input.length) {
      const two = input.slice(i, i + 2);
      if (two === '==' || two === '!=' || two === '<=' || two === '>=' || two === '&&' || two === '||') {
        tokens.push({ kind: 'operator', value: two });
        i += 2;
        continue;
      }
    }

    // Single-char operators
    if ('+-*/%=<>!'.includes(ch)) {
      tokens.push({ kind: 'operator', value: ch });
      i++;
      continue;
    }

    // Punctuation
    if ('(){}[],.'.includes(ch)) {
      tokens.push({ kind: 'punctuation', value: ch });
      i++;
      continue;
    }

    // Whitespace and other
    tokens.push({ kind: 'text', value: ch });
    i++;
  }

  return tokens;
}

import { h } from 'preact';

interface StatusBarProps {
  status: string;
  cursor: { line: number; col: number };
  wasmLoaded: boolean;
}

export function StatusBar({ status, cursor, wasmLoaded }: StatusBarProps) {
  return h('div', { class: 'status-bar' },
    h('span', { id: 'statusText' },
      status,
      wasmLoaded ? ' (WASM)' : '',
    ),
    h('span', { id: 'cursorPos' },
      `Ln ${cursor.line}, Col ${cursor.col}`,
    ),
  );
}

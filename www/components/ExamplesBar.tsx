import { h } from 'preact';
import type { Example } from '../types.ts';

interface ExamplesBarProps {
  examples: Example[];
  onSelect: (index: number) => void;
}

export function ExamplesBar({ examples, onSelect }: ExamplesBarProps) {
  return h('div', { class: 'examples', id: 'examples' },
    examples.map((ex, i) =>
      h('button', {
        class: 'example-btn',
        key: i,
        onClick: () => onSelect(i),
      }, ex.name)
    ),
  );
}

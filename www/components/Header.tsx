import { h } from 'preact';

interface HeaderProps {
  onParse: () => void;
  onRun: () => void;
  onClear: () => void;
}

export function Header({ onParse, onRun, onClear }: HeaderProps) {
  return h('header', { class: 'header' },
    h('h1', null, '.flow Playground'),
    h('span', { class: 'badge' }, 'v0.1'),
    h('div', { class: 'toolbar' },
      h('button', {
        class: 'btn primary',
        title: 'Parse .flow code',
        onClick: onParse,
      }, 'Parse'),
      h('button', {
        class: 'btn primary',
        title: 'Execute workflow',
        onClick: onRun,
      }, 'Run'),
      h('button', {
        class: 'btn',
        onClick: onClear,
      }, 'Clear'),
    ),
  );
}

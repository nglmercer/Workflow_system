import { h } from 'preact';

interface HeaderProps {
  onParse: () => void;
  onRun: () => void;
  onClear: () => void;
}

export function Header({ onParse, onRun, onClear }: HeaderProps) {
  return h('header', null,
    h('h1', null, '.flow Playground'),
    h('span', { class: 'badge' }, 'v0.1'),
    h('div', { class: 'toolbar' },
      h('button', {
        class: 'btn primary',
        id: 'btn-parse',
        title: 'Parse .flow code',
        onClick: onParse,
      }, 'Parse'),
      h('button', {
        class: 'btn primary',
        id: 'btn-run',
        title: 'Execute workflow',
        onClick: onRun,
      }, 'Run'),
      h('button', {
        class: 'btn',
        id: 'btn-clear',
        onClick: onClear,
      }, 'Clear'),
    ),
  );
}

import { h } from 'preact';
import { useRef, useEffect } from 'preact/hooks';

interface HoverTooltipProps {
  content: string;
  position: { left: number; top: number };
  visible: boolean;
}

export function HoverTooltip({ content, position, visible }: HoverTooltipProps) {
  const tooltipRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!tooltipRef.current || !visible) return;

    const tooltip = tooltipRef.current;
    const rect = tooltip.getBoundingClientRect();
    const viewportWidth = window.innerWidth;
    const viewportHeight = window.innerHeight;

    let left = position.left;
    let top = position.top - rect.height - 8;

    if (top < 0) {
      top = position.top + 24;
    }

    if (left + rect.width > viewportWidth - 16) {
      left = viewportWidth - rect.width - 16;
    }
    if (left < 16) left = 16;

    if (top + rect.height > viewportHeight - 16) {
      top = viewportHeight - rect.height - 16;
    }

    tooltip.style.left = `${left}px`;
    tooltip.style.top = `${top}px`;
  }, [content, position, visible]);

  if (!visible || !content) return null;

  const lines = content.split('\n');

  return h('div', {
    ref: tooltipRef,
    class: 'hover-tooltip',
    style: {
      left: `${position.left}px`,
      top: `${position.top - 8}px`,
    },
  },
    lines.map((line, i) => {
      if (i === 0 && (line.includes('fn ') || line.includes('var ') || line.includes('data.') || line.includes(': '))) {
        return h('div', { class: 'hover-tooltip-title', key: i }, line);
      }
      if (line.startsWith('  ')) {
        return h('div', { class: 'hover-tooltip-prop', key: i }, line.trim());
      }
      return h('div', { class: 'hover-tooltip-line', key: i }, line);
    })
  );
}

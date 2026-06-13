import { h } from 'preact';
import { useCallback, useEffect, useRef, useState } from 'preact/hooks';
import type { FlowProgram } from '@src/types.ts';
import type { SchemaType } from '../schema.ts';
import type { RefObject } from 'preact';
import { getCompletions, type CompletionItem } from '../autocomplete.ts';
import { getHoverInfo, type HoverInfo } from '../hover.ts';
import { HoverTooltip } from './HoverTooltip.tsx';

function isInsideString(text: string): boolean {
  let inString = false;
  let stringChar = '';
  let escaped = false;

  for (let i = 0; i < text.length; i++) {
    const ch = text[i];

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

function isInsideComment(text: string): boolean {
  for (let i = 0; i < text.length - 1; i++) {
    if (text[i] === '/' && text[i + 1] === '/') {
      return true;
    }
  }
  return false;
}

interface EditorProps {
  code: string;
  onChange: (value: string) => void;
  editorRef: RefObject<HTMLTextAreaElement>;
  highlightRef: RefObject<HTMLPreElement>;
  onCursorChange: (pos: { line: number; col: number }) => void;
  schema: SchemaType;
  program: FlowProgram | null;
}

export function Editor({ code, onChange, editorRef, highlightRef, onCursorChange, schema, program }: EditorProps) {
  const autocompleteRef = useRef<HTMLDivElement>(null);
  const autocompleteVisible = useRef(false);
  const autocompleteItems = useRef<CompletionItem[]>([]);
  const autocompleteIndex = useRef(0);

  const [hoverInfo, setHoverInfo] = useState<HoverInfo | null>(null);
  const [hoverPosition, setHoverPosition] = useState({ left: 0, top: 0 });
  const [hoverVisible, setHoverVisible] = useState(false);
  const hoverTimeoutRef = useRef<number | null>(null);
  const lastHoverPos = useRef(-1);
  const justInsertedRef = useRef(false);

  const syncScroll = useCallback(() => {
    if (editorRef.current && highlightRef.current) {
      highlightRef.current.scrollTop = editorRef.current.scrollTop;
      highlightRef.current.scrollLeft = editorRef.current.scrollLeft;
    }
  }, [editorRef, highlightRef]);

  const updateCursor = useCallback(() => {
    if (!editorRef.current) return;
    const val = editorRef.current.value;
    const pos = editorRef.current.selectionStart;
    const lines = val.substring(0, pos).split('\n');
    onCursorChange({
      line: lines.length,
      col: lines[lines.length - 1].length + 1,
    });
  }, [editorRef, onCursorChange]);

  const hideAutocomplete = useCallback(() => {
    autocompleteVisible.current = false;
    autocompleteItems.current = [];
    if (autocompleteRef.current) {
      autocompleteRef.current.style.display = 'none';
    }
  }, []);

  const getEditorCursorPos = useCallback(() => {
    if (!editorRef.current) return { left: 0, top: 0 };
    const text = editorRef.current.value.substring(0, editorRef.current.selectionStart);
    const lines = text.split('\n');
    const lineNum = lines.length - 1;
    const colNum = lines[lineNum].length;

    const lineHeight = 20.8;
    const charWidth = 7.8;

    return {
      left: 16 + colNum * charWidth - editorRef.current.scrollLeft,
      top: 16 + lineNum * lineHeight - editorRef.current.scrollTop,
    };
  }, [editorRef]);

  const showAutocomplete = useCallback((items: CompletionItem[]) => {
    if (items.length === 0 || !autocompleteRef.current) {
      hideAutocomplete();
      return;
    }

    autocompleteItems.current = items;
    autocompleteIndex.current = 0;
    autocompleteVisible.current = true;

    const escapeHtml = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

    autocompleteRef.current.innerHTML = items
      .map((item, i) => {
        const kindIcon = getKindIcon(item.kind);
        const detail = item.detail ? `<span class="autocomplete-detail">${escapeHtml(item.detail)}</span>` : '';
        return `<div class="autocomplete-item${i === 0 ? ' selected' : ''}" data-index="${i}"><span class="autocomplete-kind">${kindIcon}</span>${escapeHtml(item.label)}${detail}</div>`;
      })
      .join('');

    const pos = getEditorCursorPos();
    autocompleteRef.current.style.left = `${pos.left}px`;
    autocompleteRef.current.style.top = `${pos.top + 20}px`;
    autocompleteRef.current.style.display = 'block';
  }, [getEditorCursorPos, hideAutocomplete]);

  const insertAutocompleteItem = useCallback((item: CompletionItem) => {
    if (!editorRef.current) return;
    const pos = editorRef.current.selectionStart;
    const text = editorRef.current.value;
    const before = text.substring(0, pos);
    const after = text.substring(pos);

    justInsertedRef.current = true;
    setTimeout(() => { justInsertedRef.current = false; }, 100);

    let insertText = item.insertText;
    let selectStart = -1;
    let selectEnd = -1;

    const snippetMatch = insertText.match(/\$\{(\d+):([^}]*)\}/);
    if (snippetMatch) {
      const snippetParts = insertText.split(/\$\{(\d+):([^}]*)\}/);
      let cleanText = '';
      let i = 0;
      while (i < snippetParts.length) {
        cleanText += snippetParts[i];
        if (i + 2 < snippetParts.length) {
          const placeholder = snippetParts[i + 2];
          selectStart = cleanText.length;
          cleanText += placeholder;
          selectEnd = cleanText.length;
          i += 3;
        } else {
          i++;
        }
      }
      insertText = cleanText;
    }

    const dotMatch = before.match(/(\w+(?:\.\w+)*)\.(\w*)$/);
    if (dotMatch) {
      const prefix = before.substring(0, before.length - dotMatch[2].length);
      const newValue = prefix + insertText + after;
      onChange(newValue);
      if (selectStart >= 0) {
        editorRef.current.selectionStart = prefix.length + selectStart;
        editorRef.current.selectionEnd = prefix.length + selectEnd;
      } else {
        editorRef.current.selectionStart = editorRef.current.selectionEnd = prefix.length + insertText.length;
      }
    } else {
      const wordMatch = before.match(/(\w+)$/);
      if (wordMatch) {
        const wordStart = before.length - wordMatch[1].length;
        const prefix = before.substring(0, wordStart);
        const newValue = prefix + insertText + after;
        onChange(newValue);
        if (selectStart >= 0) {
          editorRef.current.selectionStart = prefix.length + selectStart;
          editorRef.current.selectionEnd = prefix.length + selectEnd;
        } else {
          editorRef.current.selectionStart = editorRef.current.selectionEnd = prefix.length + insertText.length;
        }
      } else {
        onChange(before + insertText + after);
        if (selectStart >= 0) {
          editorRef.current.selectionStart = pos + selectStart;
          editorRef.current.selectionEnd = pos + selectEnd;
        } else {
          editorRef.current.selectionStart = editorRef.current.selectionEnd = pos + insertText.length;
        }
      }
    }

    hideAutocomplete();
    editorRef.current.focus();
  }, [editorRef, onChange, hideAutocomplete]);

  const checkAutocomplete = useCallback(() => {
    if (!editorRef.current || justInsertedRef.current) return;
    const pos = editorRef.current.selectionStart;
    const text = editorRef.current.value;
    const before = text.substring(0, pos);

    const inString = isInsideString(before);
    const inComment = isInsideComment(before);

    if (inString || inComment) {
      hideAutocomplete();
      return;
    }

    const wordMatch = before.match(/(\w+)$/);
    if (wordMatch && wordMatch[1].length < 1) {
      hideAutocomplete();
      return;
    }

    const items = getCompletions(code, pos, schema, program);

    if (items.length > 0) {
      showAutocomplete(items);
    } else {
      hideAutocomplete();
    }
  }, [code, schema, program, showAutocomplete, hideAutocomplete]);

  const checkHover = useCallback((e: MouseEvent) => {
    if (!editorRef.current) return;

    const rect = editorRef.current.getBoundingClientRect();
    const x = e.clientX - rect.left + editorRef.current.scrollLeft;
    const y = e.clientY - rect.top + editorRef.current.scrollTop;

    const lineHeight = 20.8;
    const charWidth = 7.8;

    const line = Math.floor((y - 16) / lineHeight);
    const col = Math.floor((x - 16) / charWidth);

    const lines = code.split('\n');
    if (line < 0 || line >= lines.length) {
      setHoverVisible(false);
      return;
    }

    const lineText = lines[line];
    if (col < 0 || col >= lineText.length) {
      setHoverVisible(false);
      return;
    }

    let charIndex = 0;
    for (let i = 0; i < line; i++) {
      charIndex += lines[i].length + 1;
    }
    charIndex += col;

    if (charIndex === lastHoverPos.current) return;
    lastHoverPos.current = charIndex;

    const info = getHoverInfo(code, charIndex, schema, program);
    if (info) {
      setHoverInfo(info);
      setHoverPosition({
        left: e.clientX - rect.left,
        top: e.clientY - rect.top,
      });
      setHoverVisible(true);
    } else {
      setHoverVisible(false);
    }
  }, [code, schema, program, editorRef]);

  const hideHover = useCallback(() => {
    setHoverVisible(false);
    lastHoverPos.current = -1;
  }, []);

  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    if (autocompleteVisible.current) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        autocompleteIndex.current = Math.min(autocompleteIndex.current + 1, autocompleteItems.current.length - 1);
        const items = autocompleteRef.current?.querySelectorAll('.autocomplete-item');
        items?.forEach((item, i) => {
          item.classList.toggle('selected', i === autocompleteIndex.current);
        });
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        autocompleteIndex.current = Math.max(autocompleteIndex.current - 1, 0);
        const items = autocompleteRef.current?.querySelectorAll('.autocomplete-item');
        items?.forEach((item, i) => {
          item.classList.toggle('selected', i === autocompleteIndex.current);
        });
        return;
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault();
        insertAutocompleteItem(autocompleteItems.current[autocompleteIndex.current]);
        return;
      }
      if (e.key === 'Escape') {
        hideAutocomplete();
        return;
      }
    }

    if (e.key === 'Tab') {
      e.preventDefault();
      if (!editorRef.current) return;
      const start = editorRef.current.selectionStart;
      const newValue = editorRef.current.value.substring(0, start) + '  ' + editorRef.current.value.substring(editorRef.current.selectionEnd);
      onChange(newValue);
      editorRef.current.selectionStart = editorRef.current.selectionEnd = start + 2;
    }
  }, [editorRef, onChange, insertAutocompleteItem, hideAutocomplete]);

  useEffect(() => {
    const editor = editorRef.current;
    if (!editor) return;

    const onInput = () => checkAutocomplete();
    const onScroll = () => syncScroll();
    const onKeyUp = () => updateCursor();
    const onClick = () => {
      updateCursor();
      hideAutocomplete();
    };
    const onMouseMove = (e: MouseEvent) => checkHover(e);
    const onMouseLeave = () => hideHover();

    editor.addEventListener('input', onInput);
    editor.addEventListener('scroll', onScroll);
    editor.addEventListener('keyup', onKeyUp);
    editor.addEventListener('click', onClick);
    editor.addEventListener('mousemove', onMouseMove);
    editor.addEventListener('mouseleave', onMouseLeave);
    editor.addEventListener('keydown', handleKeyDown);

    return () => {
      editor.removeEventListener('input', onInput);
      editor.removeEventListener('scroll', onScroll);
      editor.removeEventListener('keyup', onKeyUp);
      editor.removeEventListener('click', onClick);
      editor.removeEventListener('mousemove', onMouseMove);
      editor.removeEventListener('mouseleave', onMouseLeave);
      editor.removeEventListener('keydown', handleKeyDown);
    };
  }, [editorRef, syncScroll, updateCursor, checkAutocomplete, checkHover, hideHover, handleKeyDown, hideAutocomplete]);

  useEffect(() => {
    return () => {
      if (hoverTimeoutRef.current) {
        clearTimeout(hoverTimeoutRef.current);
      }
    };
  }, []);

  return h('div', { class: 'editor-panel' },
    h('div', { class: 'panel-header' },
      h('span', { class: 'dot green' }),
      h('span', null, '.flow'),
    ),
    h('div', { class: 'editor-wrap' },
      h('pre', { id: 'highlight', ref: highlightRef }),
      h('textarea', {
        id: 'editor',
        ref: editorRef,
        spellcheck: false,
        placeholder: 'Write your .flow code here...',
        value: code,
        onInput: (e: Event) => onChange((e.target as HTMLTextAreaElement).value),
      }),
      h('div', {
        id: 'autocomplete',
        ref: autocompleteRef,
        class: 'autocomplete-popup',
        style: { display: 'none' },
      }),
      h(HoverTooltip, {
        content: hoverInfo?.content || '',
        position: hoverPosition,
        visible: hoverVisible,
      }),
    ),
  );
}

function getKindIcon(kind: CompletionItem['kind']): string {
  switch (kind) {
    case 'keyword': return 'K';
    case 'function': return 'F';
    case 'variable': return 'V';
    case 'property': return 'P';
    case 'snippet': return 'S';
    default: return '?';
  }
}

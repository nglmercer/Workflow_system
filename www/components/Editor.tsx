import { h } from 'preact';
import { useCallback, useEffect, useRef } from 'preact/hooks';
import type { SchemaType } from '../schema.ts';
import type { RefObject } from 'preact';

interface EditorProps {
  code: string;
  onChange: (value: string) => void;
  editorRef: RefObject<HTMLTextAreaElement>;
  highlightRef: RefObject<HTMLPreElement>;
  onCursorChange: (pos: { line: number; col: number }) => void;
  schema: SchemaType;
}

export function Editor({ code, onChange, editorRef, highlightRef, onCursorChange, schema }: EditorProps) {
  const autocompleteRef = useRef<HTMLDivElement>(null);
  const autocompleteVisible = useRef(false);
  const autocompleteItems = useRef<string[]>([]);
  const autocompleteIndex = useRef(0);

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

  const showAutocomplete = useCallback((items: string[]) => {
    if (items.length === 0 || !autocompleteRef.current) {
      hideAutocomplete();
      return;
    }

    autocompleteItems.current = items;
    autocompleteIndex.current = 0;
    autocompleteVisible.current = true;

    autocompleteRef.current.innerHTML = items
      .map((item, i) => `<div class="autocomplete-item${i === 0 ? ' selected' : ''}" data-index="${i}">${item.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')}</div>`)
      .join('');

    const pos = getEditorCursorPos();
    autocompleteRef.current.style.left = `${pos.left}px`;
    autocompleteRef.current.style.top = `${pos.top + 20}px`;
    autocompleteRef.current.style.display = 'block';
  }, [getEditorCursorPos, hideAutocomplete]);

  const insertAutocompleteItem = useCallback((item: string) => {
    if (!editorRef.current) return;
    const pos = editorRef.current.selectionStart;
    const text = editorRef.current.value;
    const before = text.substring(0, pos);
    const after = text.substring(pos);

    const dotMatch = before.match(/(\w+(?:\.\w+)*)\.(\w*)$/);
    if (dotMatch) {
      const prefix = before.substring(0, before.length - dotMatch[2].length);
      onChange(prefix + item + after);
      editorRef.current.selectionStart = editorRef.current.selectionEnd = prefix.length + item.length;
    } else {
      onChange(before + item + after);
      editorRef.current.selectionStart = editorRef.current.selectionEnd = pos + item.length;
    }

    hideAutocomplete();
    editorRef.current.focus();
  }, [editorRef, onChange, hideAutocomplete]);

  const checkAutocomplete = useCallback(() => {
    if (!editorRef.current) return;
    const pos = editorRef.current.selectionStart;
    const text = editorRef.current.value;
    const before = text.substring(0, pos);

    const dotMatch = before.match(/(\w+(?:\.\w+)*)\.$/);
    if (dotMatch) {
      const path = dotMatch[1];
      const parts = path.split('.');

      if (parts[0] === 'data' && Object.keys(schema).length > 0) {
        const dataPath = parts.slice(1).join('.');
        const subSchema = dataPath
          ? (schema[dataPath.split('.')[0]] as SchemaType)
          : schema;

        if (subSchema && typeof subSchema === 'object') {
          showAutocomplete(Object.keys(subSchema));
          return;
        }
      }
    }

    if (autocompleteVisible.current) {
      const wordMatch = before.match(/(\w+)$/);
      if (wordMatch) {
        const word = wordMatch[1].toLowerCase();
        const filtered = autocompleteItems.current.filter(item => item.toLowerCase().startsWith(word));
        if (filtered.length > 0) {
          showAutocomplete(filtered);
        } else {
          hideAutocomplete();
        }
      } else {
        hideAutocomplete();
      }
    }
  }, [editorRef, schema, showAutocomplete, hideAutocomplete]);

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

    editor.addEventListener('input', () => {
      checkAutocomplete();
    });
    editor.addEventListener('scroll', syncScroll);
    editor.addEventListener('keyup', updateCursor);
    editor.addEventListener('click', () => {
      updateCursor();
      hideAutocomplete();
    });
    editor.addEventListener('keydown', handleKeyDown);

    return () => {
      editor.removeEventListener('input', () => {
        checkAutocomplete();
      });
      editor.removeEventListener('scroll', syncScroll);
      editor.removeEventListener('keyup', updateCursor);
      editor.removeEventListener('click', () => {
        updateCursor();
        hideAutocomplete();
      });
      editor.removeEventListener('keydown', handleKeyDown);
    };
  }, [editorRef, syncScroll, updateCursor, checkAutocomplete, handleKeyDown, hideAutocomplete]);

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
    ),
  );
}

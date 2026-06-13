import { Parser } from '@src/parser.ts';
import { FlowEvaluator } from '@src/evaluator.ts';
import type { FlowProgram, WorkflowDef } from '@src/types.ts';
import { tokenize } from './highlight.ts';
import { loadSchemasFromProgram, getPropertiesAtCursor, type SchemaType } from './schema.ts';

let wasmModule: any = null;
let wasmReady = false;

async function initWasm(): Promise<void> {
  try {
    const mod = await import('./wasm/workflow_wasm.js');
    await mod.default();
    wasmModule = mod;
    wasmReady = true;
    log('WASM engine loaded', 'success');
  } catch (e) {
    console.warn('WASM not available, using TypeScript engine:', e);
    log('Using TypeScript engine (WASM not available)', 'warn');
  }
}

interface Example {
  name: string;
  code: string | null;
  eventData?: string;
}

const EXAMPLES: Example[] = [
  {
    name: 'Hello World',
    code: `workflow "Hello World" {\n  on GREET\n  log("Hello, World!")\n}`,
    eventData: `{}`
  },
  {
    name: 'With Import',
    code: `import data from "./schema.json"\n\nworkflow "User Greeting" {\n  on USER_LOGIN\n  log("Welcome, " + data.userId + "!")\n  log("Plan: " + data.plan)\n}`,
    eventData: `{"userId": "user123", "plan": "premium"}`
  },
  {
    name: 'Nested Loops',
    code: `workflow "Nested Loops" {\n  on NESTED_DATA ({users, meta})\n  log("Users: " + users.length + ", Meta: " + meta.length)\n  foreach (user in users) {\n    log("User: " + user.name)\n    foreach (order in user.orders) {\n      log("  Order: " + order.id)\n      if (order.total > 100) {\n        log("    High value order")\n      }\n    }\n  }\n}`,
    eventData: `{"users": [{"name": "Alice", "orders": [{"id": "ORD-001", "total": 250}, {"id": "ORD-002", "total": 75}]}, {"name": "Bob", "orders": [{"id": "ORD-003", "total": 500}]}], "meta": [{"key": "source", "value": "web"}]}`
  },
  {
    name: 'If/Else',
    code: `workflow "User Check" {\n  on USER_LOGIN\n  if (data.plan == "premium") {\n    log("Welcome back, premium user!")\n  } else {\n    log("Free tier user")\n  }\n}`,
    eventData: `{"userId": "user123", "plan": "premium"}`
  },
  {
    name: 'Functions',
    code: `fn double(x) {\n  return x * 2\n}\n\nfn greet(name) {\n  log("Hello, " + name + "!")\n}\n\nworkflow "With Functions" {\n  on CALCULATE\n  var result = double(data.value)\n  log("Doubled: " + result)\n  greet(data.name)\n}`,
    eventData: `{"value": 21, "name": "World"}`
  },
  {
    name: 'Batch Process',
    code: `workflow "Batch" {\n  on BATCH_START\n  var total = 0\n  foreach (item in data.items) {\n    log("Processing: " + item.name)\n    var total = total + item.amount\n  }\n  log("Total: " + total)\n}`,
    eventData: `{"items": [{"name": "Widget A", "amount": 25}, {"name": "Widget B", "amount": 50}, {"name": "Widget C", "amount": 15}]}`
  },
  {
    name: 'Complex Logic',
    code: `workflow "Fraud Check" {\n  on TRANSACTION\n  if (data.amount > 1000 && data.country != "US") {\n    log("ALERT: High value international transaction")\n    log("Amount: " + data.amount)\n  } else if (data.amount > 500) {\n    log("Review: Medium value transaction")\n  } else {\n    log("OK: Normal transaction")\n  }\n}`,
    eventData: `{"amount": 1500, "country": "UK", "userId": "user456"}`
  },
  {
    name: 'Nested Loops + JSON',
    code: null,
    eventData: `{"users": [{"name": "Alice", "orders": [{"id": "ORD-001", "total": 250}, {"id": "ORD-002", "total": 75}]}, {"name": "Bob", "orders": [{"id": "ORD-003", "total": 500}]}], "meta": [{"key": "source", "value": "web"}]}`
  }
];

const editor = document.getElementById('editor') as HTMLTextAreaElement;
const highlight = document.getElementById('highlight') as HTMLPreElement;
const output = document.getElementById('output') as HTMLDivElement;
const astOutput = document.getElementById('astOutput') as HTMLPreElement;
const eventsOutput = document.getElementById('eventsOutput') as HTMLDivElement;
const statusText = document.getElementById('statusText') as HTMLSpanElement;
const cursorPos = document.getElementById('cursorPos') as HTMLSpanElement;

let eventLog: Array<{ time: string; workflow: string; logs: string[]; data: Record<string, unknown> }> = [];
let pendingEventData: string | null = null;
let currentSchema: SchemaType = {};
let autocompleteVisible = false;
let autocompleteIndex = 0;
let autocompleteItems: string[] = [];

function escapeHtml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function highlightCode(): void {
  const code = editor.value;
  const tokens = tokenize(code);
  let html = '';
  for (const token of tokens) {
    const escaped = escapeHtml(token.value);
    if (token.kind === 'text') {
      html += escaped;
    } else {
      html += `<span class="tok-${token.kind}">${escaped}</span>`;
    }
  }
  highlight.innerHTML = html + '\n';
}

function createAutocompletePopup(): HTMLDivElement {
  const popup = document.createElement('div');
  popup.id = 'autocomplete';
  popup.className = 'autocomplete-popup';
  popup.style.display = 'none';
  document.querySelector('.editor-wrap')?.appendChild(popup);
  return popup;
}

const autocompletePopup = createAutocompletePopup();

function showAutocomplete(items: string[]): void {
  if (items.length === 0) {
    hideAutocomplete();
    return;
  }

  autocompleteItems = items;
  autocompleteIndex = 0;
  autocompleteVisible = true;

  autocompletePopup.innerHTML = items
    .map((item, i) => `<div class="autocomplete-item${i === 0 ? ' selected' : ''}" data-index="${i}">${escapeHtml(item)}</div>`)
    .join('');

  const pos = getEditorCursorPos();
  autocompletePopup.style.left = `${pos.left}px`;
  autocompletePopup.style.top = `${pos.top + 20}px`;
  autocompletePopup.style.display = 'block';
}

function hideAutocomplete(): void {
  autocompleteVisible = false;
  autocompleteItems = [];
  autocompletePopup.style.display = 'none';
}

function getEditorCursorPos(): { left: number; top: number } {
  const text = editor.value.substring(0, editor.selectionStart);
  const lines = text.split('\n');
  const lineNum = lines.length - 1;
  const colNum = lines[lineNum].length;

  const lineHeight = 20.8;
  const charWidth = 7.8;

  return {
    left: 16 + colNum * charWidth - editor.scrollLeft,
    top: 16 + lineNum * lineHeight - editor.scrollTop,
  };
}

function insertAutocompleteItem(item: string): void {
  const pos = editor.selectionStart;
  const text = editor.value;
  const before = text.substring(0, pos);
  const after = text.substring(pos);

  const dotMatch = before.match(/(\w+(?:\.\w+)*)\.(\w*)$/);
  if (dotMatch) {
    const prefix = before.substring(0, before.length - dotMatch[2].length);
    editor.value = prefix + item + after;
    editor.selectionStart = editor.selectionEnd = prefix.length + item.length;
  } else {
    editor.value = before + item + after;
    editor.selectionStart = editor.selectionEnd = pos + item.length;
  }

  hideAutocomplete();
  highlightCode();
  editor.focus();
}

function checkAutocomplete(): void {
  const pos = editor.selectionStart;
  const text = editor.value;
  const before = text.substring(0, pos);

  const dotMatch = before.match(/(\w+(?:\.\w+)*)\.$/);
  if (dotMatch) {
    const path = dotMatch[1];
    const parts = path.split('.');

    if (parts[0] === 'data' && Object.keys(currentSchema).length > 0) {
      const dataPath = parts.slice(1).join('.');
      const subSchema = dataPath
        ? (currentSchema[dataPath.split('.')[0]] as SchemaType)
        : currentSchema;

      if (subSchema && typeof subSchema === 'object') {
        showAutocomplete(Object.keys(subSchema));
        return;
      }
    }
  }

  if (autocompleteVisible) {
    const wordMatch = before.match(/(\w+)$/);
    if (wordMatch) {
      const word = wordMatch[1].toLowerCase();
      const filtered = autocompleteItems.filter(item => item.toLowerCase().startsWith(word));
      if (filtered.length > 0) {
        showAutocomplete(filtered);
      } else {
        hideAutocomplete();
      }
    } else {
      hideAutocomplete();
    }
  }
}

function syncScroll(): void {
  highlight.scrollTop = editor.scrollTop;
  highlight.scrollLeft = editor.scrollLeft;
}

function log(msg: string, type: 'info' | 'success' | 'error' | 'warn' = 'info'): void {
  const el = document.createElement('div');
  el.className = `log-entry ${type}`;
  const ts = new Date().toLocaleTimeString();
  el.innerHTML = `<span class="ts">${ts}</span><span class="msg">${escapeHtml(msg)}</span>`;
  output.appendChild(el);
  output.scrollTop = output.scrollHeight;
}

function clearOutput(): void {
  output.innerHTML = '';
  astOutput.textContent = '';
}

export function switchTab(name: string): void {
  document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
  document.querySelectorAll('.tab-content').forEach(t => (t as HTMLElement).style.display = 'none');
  const idx = name === 'output' ? 1 : name === 'ast' ? 2 : 3;
  document.querySelector(`.tab:nth-child(${idx})`)?.classList.add('active');
  const tabEl = document.getElementById(`tab-${name}`);
  if (tabEl) tabEl.style.display = '';
}

function updateEventsPanel(): void {
  if (eventLog.length === 0) {
    eventsOutput.innerHTML = '<div style="color:var(--muted)">No events processed yet.</div>';
    return;
  }
  let html = '';
  for (let i = eventLog.length - 1; i >= 0; i--) {
    const ev = eventLog[i];
    html += `<div style="margin-bottom:16px;border:1px solid var(--border);border-radius:6px;padding:12px;">`;
    html += `<div style="color:var(--accent);font-weight:600;margin-bottom:4px;">${escapeHtml(ev.workflow)}</div>`;
    html += `<div style="color:var(--muted);font-size:11px;margin-bottom:8px;">${ev.time}</div>`;
    html += `<div style="color:var(--muted);font-size:12px;margin-bottom:8px;">Data: <code style="color:var(--green)">${escapeHtml(JSON.stringify(ev.data).slice(0, 120))}</code></div>`;
    html += `<div style="font-size:12px;">`;
    for (const l of ev.logs) html += `<div style="color:var(--text);padding:1px 0;">${escapeHtml(l)}</div>`;
    html += `</div></div>`;
  }
  eventsOutput.innerHTML = html;
}

function loadExample(i: number): void {
  const ex = EXAMPLES[i];
  if (ex.code) {
    editor.value = ex.code;
    highlightCode();
  }
  if (ex.eventData) {
    pendingEventData = ex.eventData;
    eventsOutput.innerHTML = `<pre style="color:var(--muted)">Event data loaded. Click Run to execute.</pre>`;
  } else {
    pendingEventData = null;
  }
  clearOutput();
}

function loadExamples(): void {
  const examplesEl = document.getElementById('examples')!;
  EXAMPLES.forEach((ex, i) => {
    const btn = document.createElement('button');
    btn.className = 'example-btn';
    btn.textContent = ex.name;
    btn.onclick = () => loadExample(i);
    examplesEl.appendChild(btn);
  });
}

export async function parseCode(): Promise<void> {
  clearOutput();
  try {
    const code = editor.value;

    if (wasmReady && wasmModule) {
      try {
        const result = wasmModule.parseFlow(code);
        astOutput.textContent = JSON.stringify(result, null, 2);

        log('Parse successful (WASM)', 'success');
        log(`Functions: ${result.functions.length}, Workflows: ${result.workflows.length}`, 'info');
        if (result.imports.length > 0) {
          log(`Imports: ${result.imports.length}`, 'info');
          for (const name of result.imports) {
            log(`  ${name}`, 'info');
          }
        }
        for (const name of result.functions) {
          log(`  fn ${name}`, 'info');
        }
        for (const wf of result.workflows) {
          log(`  workflow "${wf.name}" on ${wf.event}`, 'info');
          if (wf.params.length > 0) {
            log(`    params: ({${wf.params.join(', ')}})`, 'info');
          }
        }
        statusText.textContent = 'Parsed OK (WASM)';
        switchTab('output');
        return;
      } catch (e) {
        log(`WASM parse error: ${(e as Error).message}, falling back to TypeScript`, 'warn');
      }
    }

    const parser = new Parser(code);
    const program = parser.parseProgram();
    astOutput.textContent = JSON.stringify(program, null, 2);

    loadSchemasFromProgram(program).then(schema => {
      currentSchema = schema;
      if (Object.keys(schema).length > 0) {
        log(`Loaded schema with ${Object.keys(schema).length} properties`, 'info');
      }
    });

    log('Parse successful (TypeScript)', 'success');
    log(`Functions: ${program.functions.length}, Workflows: ${program.workflows.length}`, 'info');
    if (program.imports.length > 0) {
      log(`Imports: ${program.imports.length}`, 'info');
      for (const imp of program.imports) {
        log(`  ${imp.name} from "${imp.path}"`, 'info');
      }
    }
    for (const fn of program.functions) {
      log(`  fn ${fn.name}(${fn.params.join(', ')})`, 'info');
    }
    for (const wf of program.workflows) {
      log(`  workflow "${wf.name}" on ${wf.event}`, 'info');
      if (wf.params.length > 0) {
        log(`    params: ({${wf.params.join(', ')}})`, 'info');
      }
    }
    statusText.textContent = 'Parsed OK';
    switchTab('output');
  } catch (e) {
    log(`Parse error: ${(e as Error).message}`, 'error');
    statusText.textContent = 'Parse Error';
  }
}

export async function executeCode(): Promise<void> {
  clearOutput();
  try {
    const code = editor.value;

    let eventData: Record<string, unknown> = { userId: 'user123', plan: 'premium' };
    if (pendingEventData) {
      try {
        eventData = JSON.parse(pendingEventData);
      } catch { /* ignore */ }
    } else {
      try {
        const edText = editor.value.match(/\/\/ event:\s*(.+)/);
        if (edText) eventData = JSON.parse(edText[1]);
      } catch { /* ignore */ }
    }

    if (wasmReady && wasmModule) {
      try {
        const results = wasmModule.executeFlow(code, JSON.stringify(eventData));
        astOutput.textContent = JSON.stringify(results, null, 2);

        log('Executing workflows (WASM)...', 'info');
        log(`Event data: ${JSON.stringify(eventData)}`, 'info');
        log('─'.repeat(40), 'info');

        for (const result of results) {
          log(`▶ workflow "${result.workflow}"`, result.success ? 'info' : 'error');
          for (const l of result.logs) {
            log(`  ${l}`, 'info');
          }
          if (result.error) {
            log(`  Error: ${result.error}`, 'error');
          }
          eventLog.push({
            time: new Date().toISOString(),
            workflow: result.workflow,
            logs: result.logs,
            data: eventData
          });
        }

        log('─'.repeat(40), 'info');
        log('Execution complete', 'success');
        statusText.textContent = 'Executed OK (WASM)';
        updateEventsPanel();
        switchTab('output');
        return;
      } catch (e) {
        log(`WASM error: ${(e as Error).message}, falling back to TypeScript`, 'warn');
      }
    }

    const parser = new Parser(code);
    const program = parser.parseProgram();
    astOutput.textContent = JSON.stringify(program, null, 2);

    const evaluator = new FlowEvaluator();
    evaluator.loadProgram(program);

    log('Executing workflows (TypeScript)...', 'info');
    log(`Event data: ${JSON.stringify(eventData)}`, 'info');
    log('─'.repeat(40), 'info');

    for (const wf of program.workflows) {
      log(`▶ workflow "${wf.name}"`, 'info');
      const logs = evaluator.executeWorkflow(wf, eventData);
      for (const l of logs) {
        log(`  ${l}`, 'info');
      }
      eventLog.push({
        time: new Date().toISOString(),
        workflow: wf.name,
        logs,
        data: eventData
      });
    }

    log('─'.repeat(40), 'info');
    log('Execution complete', 'success');
    statusText.textContent = 'Executed OK';
    updateEventsPanel();
    switchTab('output');
  } catch (e) {
    log(`Runtime error: ${(e as Error).message}`, 'error');
    statusText.textContent = 'Runtime Error';
  }
}

function updateCursor(): void {
  const val = editor.value;
  const pos = editor.selectionStart;
  const lines = val.substring(0, pos).split('\n');
  cursorPos.textContent = `Ln ${lines.length}, Col ${lines[lines.length - 1].length + 1}`;
}

function setupEditor(): void {
  editor.addEventListener('input', () => {
    highlightCode();
    checkAutocomplete();
  });
  editor.addEventListener('scroll', syncScroll);
  editor.addEventListener('keyup', updateCursor);
  editor.addEventListener('click', () => {
    updateCursor();
    hideAutocomplete();
  });

  editor.addEventListener('keydown', (e) => {
    if (autocompleteVisible) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        autocompleteIndex = Math.min(autocompleteIndex + 1, autocompleteItems.length - 1);
        updateAutocompleteSelection();
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        autocompleteIndex = Math.max(autocompleteIndex - 1, 0);
        updateAutocompleteSelection();
        return;
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault();
        insertAutocompleteItem(autocompleteItems[autocompleteIndex]);
        return;
      }
      if (e.key === 'Escape') {
        hideAutocomplete();
        return;
      }
    }

    if (e.key === 'Tab') {
      e.preventDefault();
      const start = editor.selectionStart;
      editor.value = editor.value.substring(0, start) + '  ' + editor.value.substring(editor.selectionEnd);
      editor.selectionStart = editor.selectionEnd = start + 2;
      highlightCode();
    }
  });
}

function updateAutocompleteSelection(): void {
  const items = autocompletePopup.querySelectorAll('.autocomplete-item');
  items.forEach((item, i) => {
    item.classList.toggle('selected', i === autocompleteIndex);
  });
}

function setupButtons(): void {
  document.getElementById('btn-parse')?.addEventListener('click', parseCode);
  document.getElementById('btn-run')?.addEventListener('click', executeCode);
  document.getElementById('btn-clear')?.addEventListener('click', () => {
    clearOutput();
    statusText.textContent = 'Ready';
  });

  document.querySelectorAll('.tab[data-tab]').forEach(tab => {
    tab.addEventListener('click', () => {
      const name = (tab as HTMLElement).dataset.tab!;
      switchTab(name);
    });
  });
}

function setupKeyboardShortcuts(): void {
  document.addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
      e.preventDefault();
      executeCode();
    }
    if ((e.ctrlKey || e.metaKey) && e.key === 'p') {
      e.preventDefault();
      parseCode();
    }
  });
}

async function init(): Promise<void> {
  loadExamples();
  setupEditor();
  setupButtons();
  setupKeyboardShortcuts();
  editor.value = EXAMPLES[1].code!;
  highlightCode();
  await initWasm();
}

init();

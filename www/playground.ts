import { Parser } from '@src/parser.ts';
import { FlowEvaluator } from '@src/evaluator.ts';
import type { FlowProgram, WorkflowDef } from '@src/types.ts';
import { tokenize } from './highlight.ts';

interface Example {
  name: string;
  code: string | null;
  eventData?: string;
}

const EXAMPLES: Example[] = [
  {
    name: 'Hello World',
    code: `workflow "Hello World" {\n  on GREET\n  log("Hello, World!")\n}`
  },
  {
    name: 'Nested Loops',
    code: `workflow "Nested Loops" {\n  on NESTED_DATA ({users, meta})\n  log("Users: " + users.length + ", Meta: " + meta.length)\n  foreach (user in users) {\n    log("User: " + user.name)\n    foreach (order in user.orders) {\n      log("  Order: " + order.id)\n      if (order.total > 100) {\n        log("    High value order")\n      }\n    }\n  }\n}`
  },
  {
    name: 'If/Else',
    code: `workflow "User Check" {\n  on USER_LOGIN\n  if (data.plan == "premium") {\n    log("Welcome back, premium user!")\n  } else {\n    log("Free tier user")\n  }\n}`
  },
  {
    name: 'Functions',
    code: `fn double(x) {\n  return x * 2\n}\n\nfn greet(name) {\n  log("Hello, " + name + "!")\n}\n\nworkflow "With Functions" {\n  on CALCULATE\n  var result = double(data.value)\n  log("Doubled: " + result)\n  greet(data.name)\n}`
  },
  {
    name: 'Batch Process',
    code: `workflow "Batch" {\n  on BATCH_START\n  var total = 0\n  foreach (item in data.items) {\n    log("Processing: " + item.name)\n    var total = total + item.amount\n  }\n  log("Total: " + total)\n}`
  },
  {
    name: 'Complex Logic',
    code: `workflow "Fraud Check" {\n  on TRANSACTION\n  if (data.amount > 1000 && data.country != "US") {\n    log("ALERT: High value international transaction")\n    log("Amount: " + data.amount)\n  } else if (data.amount > 500) {\n    log("Review: Medium value transaction")\n  } else {\n    log("OK: Normal transaction")\n  }\n}`
  },
  {
    name: 'Nested Loops + JSON',
    code: null,
    eventData: `{\n  "users": [\n    {\n      "name": "Alice",\n      "orders": [\n        { "id": "ORD-001", "total": 250 },\n        { "id": "ORD-002", "total": 75 }\n      ]\n    },\n    {\n      "name": "Bob",\n      "orders": [\n        { "id": "ORD-003", "total": 500 }\n      ]\n    }\n  ],\n  "meta": [\n    { "key": "source", "value": "web" }\n  ]\n}`
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

export function parseCode(): void {
  clearOutput();
  try {
    const code = editor.value;
    const parser = new Parser(code);
    const program = parser.parseProgram();
    astOutput.textContent = JSON.stringify(program, null, 2);
    log('Parse successful', 'success');
    log(`Functions: ${program.functions.length}, Workflows: ${program.workflows.length}`, 'info');
    for (const fn of program.functions) {
      log(`  fn ${fn.name}(${fn.params.join(', ')})`, 'info');
    }
    for (const wf of program.workflows) {
      log(`  workflow "${wf.name}" on ${wf.body.find(s => s.type === 'On')?.event ?? '?'}`, 'info');
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

export function executeCode(): void {
  clearOutput();
  try {
    const code = editor.value;
    const parser = new Parser(code);
    const program = parser.parseProgram();
    astOutput.textContent = JSON.stringify(program, null, 2);

    const evaluator = new FlowEvaluator();
    evaluator.loadProgram(program);

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

    log('Executing workflows...', 'info');
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
  editor.addEventListener('input', highlightCode);
  editor.addEventListener('scroll', syncScroll);
  editor.addEventListener('keyup', updateCursor);
  editor.addEventListener('click', updateCursor);

  editor.addEventListener('keydown', (e) => {
    if (e.key === 'Tab') {
      e.preventDefault();
      const start = editor.selectionStart;
      editor.value = editor.value.substring(0, start) + '  ' + editor.value.substring(editor.selectionEnd);
      editor.selectionStart = editor.selectionEnd = start + 2;
      highlightCode();
    }
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

function init(): void {
  loadExamples();
  setupEditor();
  setupButtons();
  setupKeyboardShortcuts();
  editor.value = EXAMPLES[1].code!;
  highlightCode();
}

init();

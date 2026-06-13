import { h } from 'preact';
import { useState, useEffect, useRef, useCallback } from 'preact/hooks';
import { Parser } from '@src/parser.ts';
import { FlowEvaluator } from '@src/evaluator.ts';
import { tokenize } from '../highlight.ts';
import { loadSchemasFromProgram, type SchemaType } from '../schema.ts';
import { EXAMPLES, type LogEntry, type EventLogEntry, type TabName } from '../types.ts';
import { Header } from './Header.tsx';
import { ExamplesBar } from './ExamplesBar.tsx';
import { Editor } from './Editor.tsx';
import { OutputPanel } from './OutputPanel.tsx';
import { StatusBar } from './StatusBar.tsx';

let wasmModule: any = null;
let wasmReady = false;

async function initWasm(): Promise<boolean> {
  try {
    const mod = await import('../wasm/workflow_wasm.js');
    await mod.default();
    wasmModule = mod;
    wasmReady = true;
    return true;
  } catch (e) {
    console.warn('WASM not available:', e);
    return false;
  }
}

export function App() {
  const [code, setCode] = useState(EXAMPLES[1].code || '');
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [astText, setAstText] = useState('');
  const [eventLog, setEventLog] = useState<EventLogEntry[]>([]);
  const [activeTab, setActiveTab] = useState<TabName>('output');
  const [statusText, setStatusText] = useState('Ready');
  const [cursorPos, setCursorPos] = useState({ line: 1, col: 1 });
  const [pendingEventData, setPendingEventData] = useState<string | null>(null);
  const [wasmLoaded, setWasmLoaded] = useState(false);
  const [currentSchema, setCurrentSchema] = useState<SchemaType>({});

  const editorRef = useRef<HTMLTextAreaElement>(null);
  const highlightRef = useRef<HTMLPreElement>(null);

  useEffect(() => {
    initWasm().then(setWasmLoaded);
    highlightCode(code);
  }, []);

  const addLog = useCallback((message: string, type: LogEntry['type'] = 'info') => {
    setLogs(prev => [...prev, { time: new Date().toLocaleTimeString(), message, type }]);
  }, []);

  const clearLogs = useCallback(() => {
    setLogs([]);
    setAstText('');
  }, []);

  const highlightCode = useCallback((value: string) => {
    const tokens = tokenize(value);
    let html = '';
    for (const token of tokens) {
      const escaped = token.value
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;');
      if (token.kind === 'text') {
        html += escaped;
      } else {
        html += `<span class="tok-${token.kind}">${escaped}</span>`;
      }
    }
    if (highlightRef.current) {
      highlightRef.current.innerHTML = html + '\n';
    }
  }, []);

  const handleCodeChange = useCallback((value: string) => {
    setCode(value);
    highlightCode(value);
  }, [highlightCode]);

  const loadExample = useCallback((index: number) => {
    const ex = EXAMPLES[index];
    if (ex.code) {
      setCode(ex.code);
      highlightCode(ex.code);
    }
    if (ex.eventData) {
      setPendingEventData(ex.eventData);
    } else {
      setPendingEventData(null);
    }
    clearLogs();
  }, [highlightCode, clearLogs]);

  const parseCode = useCallback(async () => {
    clearLogs();
    try {
      if (wasmReady && wasmModule) {
        try {
          const result = wasmModule.parseFlow(code);
          setAstText(JSON.stringify(result, null, 2));

          addLog('Parse successful (WASM)', 'success');
          addLog(`Functions: ${result.functions.length}, Workflows: ${result.workflows.length}`, 'info');
          if (result.imports.length > 0) {
            addLog(`Imports: ${result.imports.length}`, 'info');
            for (const name of result.imports) {
              addLog(`  ${name}`, 'info');
            }
          }
          for (const name of result.functions) {
            addLog(`  fn ${name}`, 'info');
          }
          for (const wf of result.workflows) {
            addLog(`  workflow "${wf.name}" on ${wf.event}`, 'info');
            if (wf.params.length > 0) {
              addLog(`    params: ({${wf.params.join(', ')}})`, 'info');
            }
          }
          setStatusText('Parsed OK (WASM)');
          setActiveTab('output');
          return;
        } catch (e) {
          addLog(`WASM parse error: ${(e as Error).message}, falling back to TypeScript`, 'warn');
        }
      }

      const parser = new Parser(code);
      const program = parser.parseProgram();
      setAstText(JSON.stringify(program, null, 2));

      loadSchemasFromProgram(program).then(schema => {
        setCurrentSchema(schema);
        if (Object.keys(schema).length > 0) {
          addLog(`Loaded schema with ${Object.keys(schema).length} properties`, 'info');
        }
      });

      addLog('Parse successful (TypeScript)', 'success');
      addLog(`Functions: ${program.functions.length}, Workflows: ${program.workflows.length}`, 'info');
      if (program.imports.length > 0) {
        addLog(`Imports: ${program.imports.length}`, 'info');
        for (const imp of program.imports) {
          addLog(`  ${imp.name} from "${imp.path}"`, 'info');
        }
      }
      for (const fn of program.functions) {
        addLog(`  fn ${fn.name}(${fn.params.join(', ')})`, 'info');
      }
      for (const wf of program.workflows) {
        addLog(`  workflow "${wf.name}" on ${wf.event}`, 'info');
        if (wf.params.length > 0) {
          addLog(`    params: ({${wf.params.join(', ')}})`, 'info');
        }
      }
      setStatusText('Parsed OK');
      setActiveTab('output');
    } catch (e) {
      addLog(`Parse error: ${(e as Error).message}`, 'error');
      setStatusText('Parse Error');
    }
  }, [code, addLog, clearLogs]);

  const executeCode = useCallback(async () => {
    clearLogs();
    try {
      let eventData: Record<string, unknown> = { userId: 'user123', plan: 'premium' };
      if (pendingEventData) {
        try {
          eventData = JSON.parse(pendingEventData);
        } catch { /* ignore */ }
      } else {
        try {
          const edText = code.match(/\/\/ event:\s*(.+)/);
          if (edText) eventData = JSON.parse(edText[1]);
        } catch { /* ignore */ }
      }

      if (wasmReady && wasmModule) {
        try {
          const results = wasmModule.executeFlow(code, JSON.stringify(eventData));
          setAstText(JSON.stringify(results, null, 2));

          addLog('Executing workflows (WASM)...', 'info');
          addLog(`Event data: ${JSON.stringify(eventData)}`, 'info');
          addLog('─'.repeat(40), 'info');

          const newEventLogs: EventLogEntry[] = [];
          for (const result of results) {
            addLog(`▶ workflow "${result.workflow}"`, result.success ? 'info' : 'error');
            for (const l of result.logs) {
              addLog(`  ${l}`, 'info');
            }
            if (result.error) {
              addLog(`  Error: ${result.error}`, 'error');
            }
            newEventLogs.push({
              time: new Date().toISOString(),
              workflow: result.workflow,
              logs: result.logs,
              data: eventData
            });
          }

          addLog('─'.repeat(40), 'info');
          addLog('Execution complete', 'success');
          setStatusText('Executed OK (WASM)');
          setEventLog(prev => [...newEventLogs, ...prev]);
          setActiveTab('output');
          return;
        } catch (e) {
          addLog(`WASM error: ${(e as Error).message}, falling back to TypeScript`, 'warn');
        }
      }

      const parser = new Parser(code);
      const program = parser.parseProgram();
      setAstText(JSON.stringify(program, null, 2));

      const evaluator = new FlowEvaluator();
      evaluator.loadProgram(program);

      addLog('Executing workflows (TypeScript)...', 'info');
      addLog(`Event data: ${JSON.stringify(eventData)}`, 'info');
      addLog('─'.repeat(40), 'info');

      const newEventLogs: EventLogEntry[] = [];
      for (const wf of program.workflows) {
        addLog(`▶ workflow "${wf.name}"`, 'info');
        const wfLogs = evaluator.executeWorkflow(wf, eventData);
        for (const l of wfLogs) {
          addLog(`  ${l}`, 'info');
        }
        newEventLogs.push({
          time: new Date().toISOString(),
          workflow: wf.name,
          logs: wfLogs,
          data: eventData
        });
      }

      addLog('─'.repeat(40), 'info');
      addLog('Execution complete', 'success');
      setStatusText('Executed OK');
      setEventLog(prev => [...newEventLogs, ...prev]);
      setActiveTab('output');
    } catch (e) {
      addLog(`Runtime error: ${(e as Error).message}`, 'error');
      setStatusText('Runtime Error');
    }
  }, [code, pendingEventData, addLog, clearLogs]);

  const clearOutput = useCallback(() => {
    clearLogs();
    setStatusText('Ready');
  }, [clearLogs]);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
        e.preventDefault();
        executeCode();
      }
      if ((e.ctrlKey || e.metaKey) && e.key === 'p') {
        e.preventDefault();
        parseCode();
      }
    };
    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [executeCode, parseCode]);

  return h('div', { class: 'app' },
    h(Header, {
      onParse: parseCode,
      onRun: executeCode,
      onClear: clearOutput,
    }),
    h(ExamplesBar, {
      examples: EXAMPLES,
      onSelect: loadExample,
    }),
    h('div', { class: 'main' },
      h(Editor, {
        code,
        onChange: handleCodeChange,
        editorRef,
        highlightRef,
        onCursorChange: setCursorPos,
        schema: currentSchema,
      }),
      h(OutputPanel, {
        logs,
        astText,
        eventLog,
        activeTab,
        onTabChange: setActiveTab,
      }),
    ),
    h(StatusBar, {
      status: statusText,
      cursor: cursorPos,
      wasmLoaded,
    }),
  );
}

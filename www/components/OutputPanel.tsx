import { h } from 'preact';
import type { LogEntry, EventLogEntry, TabName } from '../types.ts';

interface OutputPanelProps {
  logs: LogEntry[];
  astText: string;
  eventLog: EventLogEntry[];
  activeTab: TabName;
  onTabChange: (tab: TabName) => void;
}

function escapeHtml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

export function OutputPanel({ logs, astText, eventLog, activeTab, onTabChange }: OutputPanelProps) {
  return h('div', { class: 'output-panel' },
    h('div', { class: 'tabs' },
      h('div', {
        class: `tab ${activeTab === 'output' ? 'active' : ''}`,
        onClick: () => onTabChange('output'),
      }, 'Output'),
      h('div', {
        class: `tab ${activeTab === 'ast' ? 'active' : ''}`,
        onClick: () => onTabChange('ast'),
      }, 'AST'),
      h('div', {
        class: `tab ${activeTab === 'events' ? 'active' : ''}`,
        onClick: () => onTabChange('events'),
      }, 'Events'),
    ),
    h('div', {
      class: 'tab-content',
      id: 'tab-output',
      style: { display: activeTab === 'output' ? '' : 'none' },
    },
      h('div', { id: 'output' },
        logs.map((log, i) =>
          h('div', { class: `log-entry ${log.type}`, key: i },
            h('span', { class: 'ts' }, log.time),
            h('span', { class: 'msg' }, log.message),
          )
        ),
      ),
    ),
    h('div', {
      class: 'tab-content',
      id: 'tab-ast',
      style: { display: activeTab === 'ast' ? '' : 'none' },
    },
      h('pre', { id: 'astOutput' }, astText),
    ),
    h('div', {
      class: 'tab-content',
      id: 'tab-events',
      style: { display: activeTab === 'events' ? '' : 'none' },
    },
      h('div', { id: 'eventsOutput' },
        eventLog.length === 0
          ? h('div', { style: { color: 'var(--muted)' } }, 'No events processed yet.')
          : eventLog.map((ev, i) =>
            h('div', {
              key: i,
              style: {
                marginBottom: '16px',
                border: '1px solid var(--border)',
                borderRadius: '6px',
                padding: '12px',
              },
            },
              h('div', {
                style: {
                  color: 'var(--accent)',
                  fontWeight: '600',
                  marginBottom: '4px',
                },
              }, ev.workflow),
              h('div', {
                style: {
                  color: 'var(--muted)',
                  fontSize: '11px',
                  marginBottom: '8px',
                },
              }, ev.time),
              h('div', {
                style: {
                  color: 'var(--muted)',
                  fontSize: '12px',
                  marginBottom: '8px',
                },
              },
                'Data: ',
                h('code', { style: { color: 'var(--green)' } },
                  escapeHtml(JSON.stringify(ev.data).slice(0, 120))
                ),
              ),
              h('div', { style: { fontSize: '12px' } },
                ev.logs.map((l, j) =>
                  h('div', {
                    key: j,
                    style: {
                      color: 'var(--text)',
                      padding: '1px 0',
                    },
                  }, l)
                ),
              ),
            )
          ),
      ),
    ),
  );
}

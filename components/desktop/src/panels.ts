import type { PanelDefinition, PanelId } from './types';

export const panels: PanelDefinition[] = [
  { id: 'explorer', label: 'Explorer', icon: '▣', hint: 'Browse files and switch workspaces', defaultWidth: 660, defaultHeight: 560, minWidth: 360, minHeight: 280 },
  { id: 'chat', label: 'Chat', icon: '✦', hint: 'Active agent transcript', defaultWidth: 760, defaultHeight: 620, minWidth: 340, minHeight: 260 },
  { id: 'sessions', label: 'Sessions', icon: '▤', hint: 'Load saved work', defaultWidth: 520, defaultHeight: 420, minWidth: 320, minHeight: 240 },
  { id: 'work', label: 'Work log', icon: '◌', hint: 'Bridge and tool events', defaultWidth: 520, defaultHeight: 420, minWidth: 320, minHeight: 220 },
  { id: 'approvals', label: 'Approvals', icon: '◇', hint: 'Risk gates', defaultWidth: 520, defaultHeight: 300, minWidth: 320, minHeight: 180 },
  { id: 'tools', label: 'Tools', icon: '⌘', hint: 'Harness capabilities', defaultWidth: 520, defaultHeight: 420, minWidth: 320, minHeight: 220 },
  { id: 'providers', label: 'Providers', icon: '⬡', hint: 'Models and agents', defaultWidth: 620, defaultHeight: 500, minWidth: 360, minHeight: 260 },
  { id: 'capabilities', label: 'Capabilities', icon: '◬', hint: 'Bridge parity map', defaultWidth: 620, defaultHeight: 500, minWidth: 360, minHeight: 260 },
  { id: 'commands', label: 'Commands', icon: '⌁', hint: 'Full slash surface', defaultWidth: 620, defaultHeight: 500, minWidth: 360, minHeight: 260 },
  { id: 'runtime', label: 'Runtime', icon: '◍', hint: 'Policy and limits', defaultWidth: 520, defaultHeight: 360, minWidth: 320, minHeight: 220 },
  { id: 'openai', label: 'OpenAI bridge', icon: '◒', hint: 'Compat endpoint', defaultWidth: 520, defaultHeight: 360, minWidth: 320, minHeight: 220 },
  { id: 'diff', label: 'Diff', icon: '±', hint: 'Workspace changes', defaultWidth: 760, defaultHeight: 520, minWidth: 360, minHeight: 260 },
  { id: 'memory', label: 'Memory', icon: '◎', hint: 'CMS/ECM state', defaultWidth: 620, defaultHeight: 500, minWidth: 360, minHeight: 260 },
  { id: 'skills', label: 'Skills', icon: '✧', hint: 'Skiller/LSL workflows', defaultWidth: 620, defaultHeight: 500, minWidth: 360, minHeight: 260 },
  { id: 'integrations', label: 'Integrations', icon: '⌬', hint: 'MCP, HBSE, subagents', defaultWidth: 620, defaultHeight: 500, minWidth: 360, minHeight: 260 },
  { id: 'evidence', label: 'Evidence', icon: '◫', hint: 'Runs, trace, verify', defaultWidth: 620, defaultHeight: 500, minWidth: 360, minHeight: 260 },
  { id: 'system', label: 'System', icon: '◈', hint: 'Prompt and policy', defaultWidth: 620, defaultHeight: 500, minWidth: 360, minHeight: 260 },
  { id: 'settings', label: 'Settings', icon: '⚙', hint: 'Bridge launch config', defaultWidth: 620, defaultHeight: 460, minWidth: 360, minHeight: 260 },
];

export function panelDefinition(panelId: PanelId): PanelDefinition {
  return panels.find((panel) => panel.id === panelId) ?? panels[0];
}

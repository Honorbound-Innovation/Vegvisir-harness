import { invoke } from '@tauri-apps/api/core';
import './styles.css';

type BridgeId = string | number;

type BridgeEvent = {
  type: string;
  id?: BridgeId | null;
  payload?: any;
};

type BridgeRequest = {
  id: BridgeId;
  method: string;
  params?: Record<string, unknown>;
};

type Message = {
  role?: string;
  content?: string;
  text?: string;
  timestamp?: string;
};

type StartBridgeRequest = {
  workspace?: string;
  provider?: string;
  model?: string;
  agent?: string;
  vegvisirBinary?: string;
  dangerousBypass?: boolean;
  autoStart?: boolean;
};

type BridgeStatus = {
  running: boolean;
  pid?: number;
};

type BridgeStopResult = {
  wasRunning: boolean;
  graceful: boolean;
  killed: boolean;
  status?: string | null;
};

type BridgeMethodDraft = {
  raw: string;
  query: string;
  id: string;
  name: string;
  path: string;
  value: string;
  target: string;
  scope: string;
  limit: string;
  global: boolean;
};

type PanelId = 'chat' | 'sessions' | 'work' | 'approvals' | 'tools' | 'providers' | 'capabilities' | 'commands' | 'runtime' | 'openai' | 'diff' | 'memory' | 'skills' | 'integrations' | 'evidence' | 'system' | 'settings';

const appElement = document.querySelector<HTMLDivElement>('#app');
if (!appElement) throw new Error('missing #app root');
const app = appElement;

const panels: Array<{ id: PanelId; label: string; icon: string; hint: string }> = [
  { id: 'chat', label: 'Chat', icon: '✦', hint: 'Active agent transcript' },
  { id: 'sessions', label: 'Sessions', icon: '▤', hint: 'Load saved work' },
  { id: 'work', label: 'Work log', icon: '◌', hint: 'Bridge and tool events' },
  { id: 'approvals', label: 'Approvals', icon: '◇', hint: 'Risk gates' },
  { id: 'tools', label: 'Tools', icon: '⌘', hint: 'Harness capabilities' },
  { id: 'providers', label: 'Providers', icon: '⬡', hint: 'Models and agents' },
  { id: 'capabilities', label: 'Capabilities', icon: '◬', hint: 'Bridge parity map' },
  { id: 'commands', label: 'Commands', icon: '⌁', hint: 'Full slash surface' },
  { id: 'runtime', label: 'Runtime', icon: '◍', hint: 'Policy and limits' },
  { id: 'openai', label: 'OpenAI bridge', icon: '◒', hint: 'Compat endpoint' },
  { id: 'diff', label: 'Diff', icon: '±', hint: 'Workspace changes' },
  { id: 'memory', label: 'Memory', icon: '◎', hint: 'CMS/ECM state' },
  { id: 'skills', label: 'Skills', icon: '✧', hint: 'Skiller/LSL workflows' },
  { id: 'integrations', label: 'Integrations', icon: '⌬', hint: 'MCP, HBSE, subagents' },
  { id: 'evidence', label: 'Evidence', icon: '◫', hint: 'Runs, trace, verify' },
  { id: 'system', label: 'System', icon: '◈', hint: 'Prompt and policy' },
  { id: 'settings', label: 'Settings', icon: '⚙', hint: 'Bridge launch config' },
];

const state = {
  requestCounter: 0,
  bridgeRunning: false,
  bridgePid: null as number | null,
  bridgeStopping: false,
  autoStartAttempted: false,
  lastStopResult: null as BridgeStopResult | null,
  session: null as any,
  events: [] as BridgeEvent[],
  messages: [] as Message[],
  sessions: [] as any[],
  sessionListWorkspace: '',
  pendingAssistant: '',
  approvals: [] as any[],
  tools: [] as any[],
  providers: [] as any[],
  models: [] as any[],
  agents: [] as any[],
  commands: [] as any[],
  capabilities: null as any,
  methodOutputs: {} as Record<string, any>,
  selectedBridgeMethod: '',
  bridgeDraft: emptyBridgeMethodDraft(),
  runtimeStatus: null as any,
  hbseOnboarding: null as any,
  openaiCompat: null as any,
  diff: '',
  memory: '',
  systemPrompt: '',
  activePanel: 'chat' as PanelId,
  busy: false,
  error: '',
  settings: loadSettings(),
};

function emptyBridgeMethodDraft(): BridgeMethodDraft {
  return {
    raw: '',
    query: '',
    id: '',
    name: '',
    path: '',
    value: '',
    target: '',
    scope: '',
    limit: '',
    global: false,
  };
}

function loadSettings(): StartBridgeRequest {
  const raw = localStorage.getItem('vegvisir.desktop.settings');
  const defaults: StartBridgeRequest = {
    vegvisirBinary: 'vegvisir',
    workspace: '',
    provider: '',
    model: '',
    agent: '',
    dangerousBypass: false,
    autoStart: true,
  };
  if (!raw) return defaults;
  try {
    return { ...defaults, ...JSON.parse(raw) };
  } catch {
    return defaults;
  }
}

function saveSettings(): void {
  localStorage.setItem('vegvisir.desktop.settings', JSON.stringify(state.settings));
}

function nextId(prefix: string): string {
  state.requestCounter += 1;
  return `desktop-${prefix}-${state.requestCounter}`;
}

async function send(method: string, params: Record<string, unknown> = {}, prefix = method): Promise<void> {
  const request: BridgeRequest = { id: nextId(prefix.replace(/[^a-z0-9]+/gi, '-')), method, params };
  await invoke('bridge_send', { request });
}

async function startBridge(): Promise<void> {
  state.error = '';
  saveSettings();
  try {
    const status = await invoke<BridgeStatus>('bridge_start', { request: compactSettings() });
    await activateBridge(status);
  } catch (error) {
    state.bridgeRunning = false;
    state.bridgePid = null;
    state.error = String(error);
    state.activePanel = 'settings';
    render();
  }
}

async function restartBridge(): Promise<void> {
  state.error = '';
  state.bridgeStopping = true;
  saveSettings();
  render();
  try {
    const status = await invoke<BridgeStatus>('bridge_restart', { request: compactSettings() });
    state.bridgeStopping = false;
    state.session = null;
    state.messages = [];
    state.pendingAssistant = '';
    state.busy = false;
    await activateBridge(status);
  } catch (error) {
    state.bridgeStopping = false;
    state.bridgeRunning = false;
    state.bridgePid = null;
    state.error = String(error);
    state.activePanel = 'settings';
    render();
  }
}

async function activateBridge(status: BridgeStatus): Promise<void> {
  state.bridgeRunning = status.running;
  state.bridgePid = status.pid ?? null;
  if (!status.running) return;
  render();
  await send('initialize', {}, 'initialize');
  await refreshEverything();
}

function compactSettings(): StartBridgeRequest {
  const result: StartBridgeRequest = {};
  for (const [key, value] of Object.entries(state.settings)) {
    if (key === 'autoStart') continue;
    if (typeof value === 'string' && value.trim() !== '') (result as any)[key] = value.trim();
    else if (typeof value === 'boolean') (result as any)[key] = value;
  }
  return result;
}

async function stopBridge(): Promise<void> {
  state.bridgeStopping = true;
  render();
  try {
    const result = await invoke<BridgeStopResult>('bridge_stop');
    state.lastStopResult = result;
    state.events.push({ type: 'desktop.bridge.stopped', payload: result });
  } catch (error) {
    state.error = String(error);
  }
  state.bridgeStopping = false;
  state.bridgeRunning = false;
  state.bridgePid = null;
  state.session = null;
  state.messages = [];
  state.pendingAssistant = '';
  state.busy = false;
  render();
}

async function refreshStatus(): Promise<void> {
  try {
    const status = await invoke<BridgeStatus>('bridge_status');
    state.bridgeRunning = status.running;
    state.bridgePid = status.pid ?? null;
  } catch (error) {
    state.bridgeRunning = false;
    state.bridgePid = null;
    state.error = String(error);
  }
}

async function refreshEverything(): Promise<void> {
  if (!state.bridgeRunning) return;
  await Promise.allSettled([
    send('session.status', {}, 'status'),
    send('session.messages', {}, 'messages'),
    send('session.list', {}, 'sessions'),
    send('approvals.list', {}, 'approvals'),
    send('tools.list', {}, 'tools'),
    send('providers.list', {}, 'providers'),
    send('models.list', {}, 'models'),
    send('agents.list', {}, 'agents'),
    send('commands.list', {}, 'commands'),
    send('bridge.capabilities', {}, 'capabilities'),
    send('runtime.status', {}, 'runtime'),
    send('hbse.onboarding.providers', {}, 'hbse'),
    send('openai.compat.info', {}, 'openai'),
    send('memory.status', {}, 'memory'),
  ]);
}

async function pollBridge(): Promise<void> {
  if (!state.bridgeRunning) {
    await refreshStatus();
    if (state.autoStartAttempted) render();
    return;
  }
  try {
    const lines = await invoke<string[]>('bridge_poll');
    for (const line of lines) {
      try { handleEvent(JSON.parse(line)); }
      catch { handleEvent({ type: 'bridge.raw', payload: { line } }); }
    }
    await refreshStatus();
    if (lines.length) render();
  } catch (error) {
    state.error = String(error);
    render();
  }
}

function handleEvent(event: BridgeEvent): void {
  state.events.push(event);
  if (state.events.length > 600) state.events.splice(0, state.events.length - 600);
  const bridgedMethod = event.payload?.method;
  if (typeof bridgedMethod === 'string' && bridgedMethod.trim()) {
    state.methodOutputs[bridgedMethod] = event.payload;
  }

  switch (event.type) {
    case 'desktop.bridge.spawned':
      break;
    case 'desktop.bridge.exited':
      state.bridgeRunning = false;
      state.bridgePid = null;
      state.busy = false;
      state.error = `Bridge exited: ${event.payload?.status ?? 'unknown status'}`;
      break;
    case 'desktop.bridge.error':
      state.bridgeRunning = false;
      state.bridgePid = null;
      state.busy = false;
      state.error = event.payload?.message ?? 'bridge error';
      break;
    case 'server.ready':
    case 'session.status':
    case 'session.started':
      state.session = event.payload;
      break;
    case 'session.messages':
      state.messages = normalizeMessages(event.payload?.messages ?? event.payload ?? []);
      state.pendingAssistant = '';
      break;
    case 'session.list':
      state.sessions = Array.isArray(event.payload?.sessions) ? event.payload.sessions : [];
      state.sessionListWorkspace = event.payload?.workspace ?? '';
      break;
    case 'session.loaded':
      state.session = event.payload?.session ?? state.session;
      state.pendingAssistant = '';
      state.error = '';
      state.activePanel = 'chat';
      state.messages.push({ role: 'system', content: event.payload?.output ?? 'Session loaded.' });
      void send('session.messages', {}, 'messages');
      void send('session.status', {}, 'status');
      void send('session.list', {}, 'sessions');
      break;
    case 'turn.started':
      state.busy = true;
      state.pendingAssistant = '';
      break;
    case 'content.delta':
      state.pendingAssistant += event.payload?.text ?? '';
      break;
    case 'turn.completed':
      state.busy = false;
      if (state.pendingAssistant.trim()) state.messages.push({ role: 'assistant', content: state.pendingAssistant });
      state.pendingAssistant = '';
      void send('session.messages', {}, 'messages');
      void send('session.status', {}, 'status');
      break;
    case 'turn.failed':
      state.busy = false;
      state.error = eventMessage(event, 'turn failed');
      void send('approvals.list', {}, 'approvals');
      break;
    case 'approval.required':
      void send('approvals.list', {}, 'approvals');
      break;
    case 'approvals.list':
    case 'approvals.updated':
      state.approvals = event.payload?.approvals ?? [];
      break;
    case 'tools.list':
      state.tools = event.payload?.tools ?? [];
      break;
    case 'providers.list':
      state.providers = event.payload?.providers ?? event.payload?.availability ?? [];
      break;
    case 'models.list':
    case 'model.list':
      state.models = event.payload?.models ?? [];
      break;
    case 'agents.list':
      state.agents = event.payload?.agents ?? [];
      break;
    case 'commands.list':
      state.commands = event.payload?.commands ?? [];
      break;
    case 'bridge.capabilities':
      state.capabilities = event.payload ?? null;
      break;
    case 'runtime.status':
      state.runtimeStatus = event.payload ?? null;
      break;
    case 'hbse.onboarding.providers':
      state.hbseOnboarding = event.payload ?? null;
      break;
    case 'openai.compat.info':
      state.openaiCompat = event.payload ?? null;
      break;
    case 'command.completed':
      state.session = event.payload?.session ?? state.session;
      state.messages.push({
        role: 'command',
        content: `${event.payload?.command ?? 'command'}\n\n${event.payload?.output ?? ''}`.trim(),
      });
      state.pendingAssistant = '';
      void send('session.status', {}, 'status');
      void send('session.list', {}, 'sessions');
      break;
    case 'provider.selected':
    case 'model.selected':
    case 'agent.selected':
    case 'effort.updated':
    case 'fast.updated':
    case 'toolLimit.updated':
      state.session = event.payload?.session ?? state.session;
      void refreshEverything();
      break;
    case 'diff.current':
      state.diff = event.payload?.diff ?? event.payload?.markdown ?? event.payload?.output ?? JSON.stringify(event.payload, null, 2);
      break;
    case 'memory.status':
      state.memory = event.payload?.output ?? event.payload?.status ?? JSON.stringify(event.payload, null, 2);
      break;
    case 'system.prompt':
      state.systemPrompt = event.payload?.prompt ?? event.payload?.system_prompt ?? JSON.stringify(event.payload, null, 2);
      break;
    case 'error':
      state.error = eventMessage(event, JSON.stringify(event.payload));
      break;
  }
}

function eventMessage(event: BridgeEvent, fallback: string): string {
  const payload = event.payload;
  if (!payload || typeof payload !== 'object') return fallback;
  const direct = payload.message ?? payload.error;
  if (typeof direct === 'string' && direct.trim()) return direct;
  if (direct && typeof direct === 'object') {
    const nested = direct.message ?? direct.error;
    if (typeof nested === 'string' && nested.trim()) return nested;
  }
  return fallback;
}

function normalizeMessages(value: any[]): Message[] {
  if (!Array.isArray(value)) return [];
  return value.map((item) => {
    if (typeof item === 'string') return { role: 'message', content: item };
    return {
      role: item.role ?? item.kind ?? item.author ?? 'message',
      content: item.content ?? item.text ?? item.markdown ?? JSON.stringify(item, null, 2),
      timestamp: item.timestamp ?? item.created_at,
    };
  });
}

async function sendTurn(): Promise<void> {
  const input = document.querySelector<HTMLTextAreaElement>('#turn-input');
  const content = input?.value.trim() ?? '';
  if (!content || !state.bridgeRunning || state.busy) return;
  input!.value = '';
  state.error = '';
  if (content.startsWith('/')) {
    state.messages.push({ role: 'command', content });
    render();
    await send('command.invoke', { command: content }, 'command');
    return;
  }
  state.messages.push({ role: 'user', content });
  render();
  await send('turn.send', { content }, 'turn');
}

async function loadSession(id: string): Promise<void> {
  if (!id.trim() || !state.bridgeRunning || state.busy) return;
  await send('session.load', { id: id.trim() }, 'session-load');
}

function setPanel(panel: string): void {
  state.activePanel = panel as PanelId;
  if (panel === 'sessions') void send('session.list', {}, 'sessions');
  if (panel === 'diff') void send('diff.current', {}, 'diff');
  if (panel === 'memory') void send('memory.status', {}, 'memory');
  if (panel === 'system') void send('system.prompt', {}, 'system');
  if (panel === 'commands') void send('commands.list', {}, 'commands');
  if (panel === 'capabilities') void send('bridge.capabilities', {}, 'capabilities');
  if (panel === 'runtime') void send('runtime.status', {}, 'runtime');
  if (panel === 'openai') void send('openai.compat.info', {}, 'openai');
  if (panel === 'skills') void send('skills.status', {}, 'skills');
  if (panel === 'integrations') {
    void send('mcp.status', {}, 'mcp');
    void send('hbse.status', {}, 'hbse');
    void send('subagents.list', {}, 'subagents');
  }
  if (panel === 'evidence') {
    void send('runs.list', {}, 'runs');
    void send('trace.list', {}, 'trace');
  }
  render();
}

async function approve(id: string, method: string): Promise<void> {
  await send(method, { id }, 'approval');
  await send('approvals.list', {}, 'approvals');
}

async function selectProvider(provider: string): Promise<void> {
  await send('provider.select', { provider }, 'provider');
}

async function selectModel(model: string): Promise<void> {
  await send('model.select', { model }, 'model');
}

async function selectAgent(agent: string): Promise<void> {
  await send('agent.select', { agent }, 'agent');
}

async function runCommandValue(command: string): Promise<void> {
  if (!command.trim()) return;
  await send('command.invoke', { command }, 'command');
}

async function callBridgeMethod(method: string, params: Record<string, unknown> = {}): Promise<void> {
  if (!method.trim() || !state.bridgeRunning) return;
  state.selectedBridgeMethod = method;
  await send(method, params, method);
}

async function callBridgeMethodFromWorkbench(method: string): Promise<void> {
  state.selectedBridgeMethod = method;
  const params = bridgeMethodParamsFromDraft();
  await callBridgeMethod(method, params);
}

function bridgeMethodParamsFromDraft(): Record<string, unknown> {
  const draft = state.bridgeDraft;
  const params: Record<string, unknown> = {};
  for (const key of ['raw', 'query', 'id', 'name', 'path', 'value', 'target', 'scope'] as const) {
    const value = draft[key].trim();
    if (value) params[key] = value;
  }
  const limit = Number.parseInt(draft.limit, 10);
  if (Number.isFinite(limit) && limit > 0) params.limit = limit;
  if (draft.global) params.global = true;
  return params;
}

async function setFastMode(enabled: boolean): Promise<void> {
  await send('fast.set', { enabled }, 'fast');
}

async function setEffort(effort: string): Promise<void> {
  await send('effort.set', { effort }, 'effort');
}

async function setToolLimit(value: string): Promise<void> {
  await send('toolLimit.set', { value }, 'tool-limit');
}

function render(): void {
  app.innerHTML = `
    <div class="grid h-screen grid-cols-[18rem_minmax(0,1fr)] overflow-hidden bg-vv-bg bg-vv-radial text-vv-text selection:bg-vv-cyan/25 max-[980px]:grid-cols-1">
      ${renderLeftRail()}
      <main class="grid min-h-0 min-w-0 grid-rows-[3.75rem_minmax(0,1fr)_1.75rem] overflow-hidden border-l border-vv-line bg-vv-bg2/74 max-[980px]:border-l-0">
        ${renderTopBar()}
        <section class="flex min-h-0 flex-col overflow-hidden bg-vv-grid [background-size:42px_42px]">
          ${state.error ? renderError() : ''}
          ${renderPanel()}
        </section>
        ${renderFooterRail()}
      </main>
    </div>
  `;
  bindEvents();
}

function renderLeftRail(): string {
  return `
    <aside class="grid min-h-0 grid-rows-[auto_minmax(0,1fr)_auto] bg-vv-rail/95 px-3.5 py-3.5 shadow-[inset_-1px_0_0_rgba(255,255,255,0.06)] max-[980px]:hidden">
      <div>
        <div class="mb-4 flex items-center gap-2.5">
          <div class="grid h-8 w-8 place-items-center rounded-xl border border-vv-line bg-white/[0.045] text-sm font-black text-vv-cyan">V</div>
          <div class="text-base font-black tracking-tight">Vegvisir <span class="text-vv-muted">Desktop</span></div>
          <span class="rounded-full border border-vv-line px-1.5 py-0.5 text-[0.56rem] font-bold uppercase tracking-[0.24em] text-vv-muted">alpha</span>
        </div>
        <div class="mb-3 flex items-center justify-between text-xs">
          <div class="flex items-center gap-2 font-bold"><span class="grid h-5 w-5 place-items-center rounded-md bg-white text-vv-bg text-[0.65rem]">V</span>${escapeHtml(projectName())}</div>
          <span class="text-vv-muted">⌄</span>
        </div>
      </div>
      <div class="vv-scrollbar min-h-0 space-y-0.5 overflow-auto pr-1">
        ${panels.map(renderPanelButton).join('')}
      </div>
      <div class="space-y-2 pt-2">
        <button id="start-stop" class="vv-action w-full ${state.bridgeRunning ? '' : 'vv-action-primary'}">${state.bridgeRunning ? 'Stop bridge' : 'Start bridge'}</button>
        <button class="vv-action w-full border-dashed text-vv-muted" data-panel="settings">+ Configure workspace</button>
      </div>
    </aside>
  `;
}

function renderPanelButton(panel: { id: PanelId; label: string; icon: string; hint: string }): string {
  const badge = panel.id === 'approvals' && state.approvals.length ? `<span class="ml-auto rounded-full bg-vv-pink px-2 py-0.5 text-xs font-bold text-white">${state.approvals.length}</span>` : '';
  const active = state.activePanel === panel.id ? 'vv-rail-button-active' : '';
  return `
    <button class="vv-rail-button ${active}" data-panel="${panel.id}">
      <span class="grid h-6 w-6 shrink-0 place-items-center rounded-lg border border-vv-line bg-white/[0.035] text-vv-cyan">${panel.icon}</span>
      <span class="min-w-0"><span class="block font-semibold text-current">${escapeHtml(panel.label)}</span><span class="block truncate text-xs text-vv-dim">${escapeHtml(panel.hint)}</span></span>
      ${badge}
    </button>
  `;
}

function renderTopBar(): string {
  return `
    <header class="flex min-w-0 items-center justify-between gap-3 border-b border-vv-line bg-black/20 px-5 backdrop-blur-xl">
      <div class="min-w-0">
        <div class="flex items-center gap-3">
          <h1 class="truncate text-[1.04rem] font-black tracking-tight">${escapeHtml(activeTitle())}</h1>
          <span class="rounded-md border border-vv-line bg-white/[0.045] px-2 py-0.5 font-mono text-[0.7rem] text-vv-muted">${escapeHtml(projectName())}</span>
        </div>
        <div class="mt-0.5 flex min-w-0 items-center gap-2 text-[0.7rem] text-vv-muted">
          <span>${escapeHtml(state.settings.provider || state.session?.provider || 'default provider')}</span>
          <span>•</span><span>${escapeHtml(state.settings.model || state.session?.model || 'default model')}</span>
          <span>•</span><span>${state.events.length} events</span>
        </div>
      </div>
      <div class="flex shrink-0 items-center gap-1.5">
        <button class="vv-action" data-panel="sessions">Sessions</button>
        <button class="vv-action" id="refresh-all">Refresh</button>
        <button class="vv-action" id="restart-bridge" ${state.bridgeStopping ? 'disabled' : ''}>${state.bridgeStopping ? 'Restarting…' : 'Restart bridge'}</button>
        <button class="vv-action" data-panel="approvals">Approvals</button>
        <button class="vv-action vv-action-primary" data-panel="diff">Open diff</button>
        <div class="vv-pill ${state.bridgeRunning ? 'text-vv-green' : 'text-vv-red'}"><span class="h-2 w-2 rounded-full ${state.bridgeRunning ? 'bg-vv-green' : 'bg-vv-red'}"></span>${state.bridgeRunning ? `Bridge online${state.bridgePid ? ` · ${state.bridgePid}` : ''}` : 'Bridge offline'}</div>
      </div>
    </header>
  `;
}

function renderFooterRail(): string {
  return `
    <footer class="flex items-center justify-between border-t border-vv-line bg-black/20 px-5 font-mono text-[0.66rem] text-vv-muted">
      <span>${state.bridgeRunning ? 'Local bridge active' : 'Local bridge offline'}</span>
      <span>${escapeHtml(state.settings.workspace || 'workspace defaults to home/current dir')}</span>
      <span>${state.busy ? 'working' : 'ready'} · main</span>
    </footer>
  `;
}

function renderError(): string {
  return `<div class="mx-auto mt-3 max-w-5xl rounded-xl border border-vv-red/45 bg-vv-red/10 p-3 text-red-100 shadow-danger"><div class="flex flex-wrap items-center justify-between gap-2"><strong>Bridge problem</strong><button class="vv-action vv-action-danger" id="restart-bridge-from-error">Restart bridge</button></div><pre class="vv-code mt-2 whitespace-pre-wrap">${escapeHtml(state.error)}</pre></div>`;
}

function renderPanel(): string {
  if (state.activePanel === 'chat') return `<div class="min-h-0 flex-1 overflow-hidden">${renderChat()}</div>`;
  return `<div class="vv-scrollbar min-h-0 flex-1 overflow-auto px-4 py-3"><div class="mx-auto max-w-5xl">${renderNonChatPanel()}</div></div>`;
}

function renderNonChatPanel(): string {
  switch (state.activePanel) {
    case 'sessions': return renderSessions();
    case 'work': return renderWork();
    case 'approvals': return renderApprovals();
    case 'tools': return renderTools();
    case 'providers': return renderProviders();
    case 'capabilities': return renderCapabilities();
    case 'commands': return renderCommands();
    case 'runtime': return renderRuntime();
    case 'openai': return renderOpenAiBridge();
    case 'diff': return renderPre(state.diff || 'No diff loaded.');
    case 'memory': return renderMemoryWorkbench();
    case 'skills': return renderSkillsWorkbench();
    case 'integrations': return renderIntegrationsWorkbench();
    case 'evidence': return renderEvidenceWorkbench();
    case 'system': return renderSystem();
    case 'settings': return renderSettings();
    default: return renderChat();
  }
}

function renderChat(): string {
  const messages = [...state.messages];
  if (state.pendingAssistant) messages.push({ role: 'assistant', content: state.pendingAssistant });
  return `
    <div class="grid h-full max-h-full min-h-0 grid-rows-[minmax(0,1fr)_auto] overflow-hidden">
      <div id="chat-scroll-surface" class="vv-scrollbar min-h-0 overflow-y-auto overflow-x-hidden px-5 py-5">
        <div class="mx-auto max-w-4xl space-y-5 pb-4">
          ${messages.length ? messages.map(renderMessage).join('') : renderEmptyTranscript()}
        </div>
      </div>
      <div id="chat-composer-surface" class="max-h-[13rem] min-h-0 overflow-hidden border-t border-vv-line bg-vv-bg2/70 px-5 py-3 backdrop-blur-xl">
        ${renderComposer()}
      </div>
    </div>
  `;
}

function renderEmptyTranscript(): string {
  return `
    <div class="pt-8 text-center text-vv-muted">
      <div class="mx-auto mb-4 grid h-12 w-12 place-items-center rounded-3xl border border-vv-line bg-white/[0.035] text-2xl text-vv-cyan shadow-glow">V</div>
      <h2 class="text-xl font-black text-vv-text">Vegvisir bridge workbench</h2>
      <p class="mx-auto mt-2 max-w-2xl text-sm leading-6">The desktop app auto-starts the same harness bridge used by the TUI. Ask it to inspect, patch, test, document, route skills, or run slash commands. Same beast. Better glass box.</p>
    </div>
  `;
}

function renderComposer(): string {
  return `
    <div class="mx-auto w-full max-w-4xl">
      <div class="rounded-[1.15rem] border border-vv-line2 bg-vv-panel/90 p-2.5 shadow-[0_18px_56px_rgba(0,0,0,0.30)]">
        <textarea id="turn-input" class="vv-focus vv-scrollbar h-16 max-h-16 min-h-16 w-full resize-none rounded-xl border border-transparent bg-transparent px-2.5 py-1.5 text-[0.92rem] leading-6 text-vv-text placeholder:text-vv-dim" placeholder="Ask Vegvisir anything, or type /sessions, /load <id>, /tools, /diff..." ${state.bridgeRunning ? '' : 'disabled'}></textarea>
        <div class="mt-1.5 flex items-center justify-between gap-2 border-t border-vv-line pt-2">
          <div class="flex min-w-0 items-center gap-1.5 overflow-hidden text-xs text-vv-muted">
            <span class="vv-pill">${escapeHtml(state.settings.model || 'model default')}</span>
            <span class="vv-pill">${state.busy ? 'High activity' : 'Ready'}</span>
            <span class="vv-pill">Chat + slash commands</span>
            <span class="vv-pill">${state.settings.dangerousBypass ? 'Bypass startup' : 'Policy gated'}</span>
          </div>
          <button id="send-turn" class="vv-focus grid h-9 w-9 shrink-0 place-items-center rounded-full ${state.busy ? 'bg-vv-red' : 'bg-vv-pink'} text-lg font-black text-white shadow-[0_0_28px_rgba(255,46,126,0.28)]" ${state.bridgeRunning && !state.busy ? '' : 'disabled'}>${state.busy ? '■' : '➤'}</button>
        </div>
      </div>
      <div class="mt-2 flex flex-wrap items-center justify-between gap-2 text-xs text-vv-muted">
        <span>Slash commands run from this same input. Press <kbd class="rounded border border-vv-line px-1 text-vv-dim">Enter</kbd> to send, <kbd class="rounded border border-vv-line px-1 text-vv-dim">Shift+Enter</kbd> for newline.</span>
        <button class="vv-action" data-panel="sessions" ${state.bridgeRunning ? '' : 'disabled'}>Load session</button>
      </div>
    </div>
  `;
}

function renderMessage(message: Message): string {
  const role = message.role ?? 'message';
  const isUser = role === 'user';
  const isCommand = role === 'command';
  const isTool = role.includes('tool') || role.includes('event');
  const cardClass = isUser ? 'ml-auto max-w-3xl bg-white/[0.07]' : isCommand ? 'max-w-4xl border-vv-cyan/35 bg-vv-cyan/5' : isTool ? 'max-w-4xl border-vv-line bg-black/18 opacity-75' : 'max-w-4xl bg-white/[0.035]';
  return `
    <article class="vv-soft-panel ${cardClass}">
      <header class="flex items-center gap-3 border-b border-vv-line px-4 py-2 text-[0.68rem] uppercase tracking-[0.22em] text-vv-muted"><span class="h-2 w-2 rounded-full ${isUser ? 'bg-vv-green' : isCommand ? 'bg-vv-cyan' : 'bg-vv-cyan'}"></span>${escapeHtml(role)}</header>
      <pre class="vv-code whitespace-pre-wrap break-words px-4 py-3">${escapeHtml(message.content ?? message.text ?? '')}</pre>
    </article>
  `;
}

function renderSessions(): string {
  const sessions = Array.isArray(state.sessions) ? state.sessions : [];
  return `
    <div class="space-y-4">
      <section class="vv-panel p-4">
        <div class="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h2 class="text-base font-black">Session loader</h2>
            <p class="mt-2 text-xs leading-5 text-vv-muted">Load saved Vegvisir sessions for the active workspace without leaving the desktop UI. This uses the same harness session store and <code class="text-vv-cyan">/load</code> path as the TUI.</p>
          </div>
          <button id="refresh-sessions" class="vv-action" ${state.bridgeRunning ? '' : 'disabled'}>Refresh sessions</button>
        </div>
        <div class="mt-3 rounded-xl border border-vv-line bg-black/18 p-3 text-xs text-vv-muted">Workspace: <span class="font-mono text-vv-text">${escapeHtml(state.sessionListWorkspace || state.session?.workspace || state.settings.workspace || 'current')}</span></div>
      </section>
      <section class="grid gap-3">
        ${sessions.length ? sessions.map(renderSessionCard).join('') : '<div class="vv-panel p-4 text-sm text-vv-muted">No saved sessions loaded for this workspace. Click refresh or type <code class="text-vv-cyan">/sessions</code> in chat.</div>'}
      </section>
    </div>`;
}

function renderSessionCard(session: any): string {
  const id = String(session.session_id ?? session.id ?? '');
  const title = String(session.title ?? 'untitled');
  const current = Boolean(session.current);
  const messages = Number(session.message_count ?? 0);
  const created = formatTimestamp(session.created_at);
  return `
    <article class="vv-soft-panel p-4 ${current ? 'border-vv-cyan/45 bg-vv-cyan/5' : ''}">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div class="min-w-0">
          <div class="flex flex-wrap items-center gap-2">
            <h3 class="truncate text-sm font-black text-vv-text">${escapeHtml(title)}</h3>
            ${current ? '<span class="vv-pill text-vv-cyan">Current</span>' : ''}
          </div>
          <div class="mt-2 flex flex-wrap gap-2 text-xs text-vv-muted">
            <span class="vv-pill">${messages} messages</span>
            <span class="vv-pill">${escapeHtml(session.provider ?? 'provider')}</span>
            <span class="vv-pill">${escapeHtml(session.model ?? 'model')}</span>
            <span class="vv-pill">${escapeHtml(created)}</span>
          </div>
          <pre class="vv-code mt-3 truncate text-vv-dim">${escapeHtml(id)}</pre>
        </div>
        <button class="vv-action vv-action-primary" data-session-load="${escapeHtml(id)}" ${state.bridgeRunning && !state.busy && !current ? '' : 'disabled'}>${current ? 'Loaded' : 'Load'}</button>
      </div>
    </article>`;
}

function formatTimestamp(value: unknown): string {
  if (typeof value !== 'string' || !value.trim()) return 'unknown time';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function renderWork(): string {
  return `<div class="space-y-3">${state.events.slice().reverse().map((event) => `
    <article class="vv-soft-panel p-3 opacity-80">
      <div class="mb-2 flex items-center gap-2 font-mono text-[0.68rem] uppercase tracking-[0.2em] text-vv-muted"><span class="h-2 w-2 rounded-full bg-vv-cyan"></span>${escapeHtml(event.type)}</div>
      <pre class="vv-code whitespace-pre-wrap break-words rounded-xl bg-black/18 p-3">${escapeHtml(JSON.stringify(event.payload ?? {}, null, 2))}</pre>
    </article>
  `).join('') || '<p class="text-vv-muted">No bridge events yet.</p>'}</div>`;
}

function renderApprovals(): string {
  if (!state.approvals.length) return '<div class="vv-panel p-4 text-sm text-vv-muted">No pending approvals. The beast is behaving.</div>';
  return `<div class="grid gap-4">${state.approvals.map((approval) => `
    <article class="rounded-[1.05rem] border border-vv-red/45 bg-vv-red/10 p-4 shadow-danger">
      <h3 class="text-base font-black text-red-100">${escapeHtml(approval.tool_name ?? approval.toolName ?? 'approval')}</h3>
      <p class="mt-2 text-sm text-red-100/75">${escapeHtml(approval.reason ?? approval.risk_label ?? 'Risky action requires approval.')}</p>
      <pre class="vv-code mt-3 whitespace-pre-wrap rounded-xl bg-black/20 p-3">${escapeHtml(JSON.stringify(approval.args ?? {}, null, 2))}</pre>
      <div class="mt-3 flex flex-wrap gap-2">
        <button class="vv-action" data-approval="${escapeHtml(approval.id)}" data-method="approvals.approveOnce">Approve once</button>
        <button class="vv-action" data-approval="${escapeHtml(approval.id)}" data-method="approvals.approveSession">Approve session</button>
        <button class="vv-action vv-action-danger" data-approval="${escapeHtml(approval.id)}" data-method="approvals.deny">Deny</button>
      </div>
    </article>`).join('')}</div>`;
}

function renderTools(): string {
  return `<div class="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">${state.tools.map((tool) => `
    <article class="vv-panel p-4"><div class="mb-2 flex items-center justify-between gap-2"><h3 class="font-black">${escapeHtml(tool.name)}</h3><small class="vv-pill ${tool.risky ? 'text-vv-amber' : 'text-vv-green'}">${tool.risky ? 'risky' : 'standard'}</small></div><p class="text-xs leading-5 text-vv-muted">${escapeHtml(tool.description ?? '')}</p></article>
  `).join('') || '<p class="text-vv-muted">Tool inventory not loaded.</p>'}</div>`;
}

function renderProviders(): string {
  const providers = Array.isArray(state.providers) ? state.providers : [];
  const models = Array.isArray(state.models) ? state.models : [];
  const agents = Array.isArray(state.agents) ? state.agents : [];
  return `
    <div class="space-y-4">
      <div class="vv-panel p-4">
        <h2 class="mb-2 text-base font-black">Provider / model control</h2>
        <p class="mb-3 text-xs leading-5 text-vv-muted">Selections are sent through Vegvisir bridge methods. The desktop app never calls OpenAI directly and never handles plaintext provider secrets.</p>
        <div class="grid gap-3 lg:grid-cols-3">
          <section>${renderSelectorCards('Providers', providers, 'provider')}</section>
          <section>${renderSelectorCards('Models', models, 'model')}</section>
          <section>${renderSelectorCards('Agents', agents, 'agent')}</section>
        </div>
      </div>
      <div class="grid gap-4 lg:grid-cols-2">
        <section><h2 class="mb-3 text-base font-black">Provider JSON</h2>${renderPre(JSON.stringify(state.providers, null, 2))}</section>
        <section><h2 class="mb-3 text-base font-black">Model / agent JSON</h2>${renderPre(JSON.stringify({ models: state.models, agents: state.agents }, null, 2))}</section>
      </div>
    </div>`;
}

function renderSelectorCards(title: string, items: any[], kind: 'provider' | 'model' | 'agent'): string {
  const currentProvider = state.session?.provider ?? state.session?.current_provider;
  const currentModel = state.session?.model ?? state.session?.current_model;
  const activeAgent = state.session?.agent ?? state.session?.active_agent;
  const visible = items.slice(0, 24);
  const cards = visible.map((item) => {
    const id = String(item.name ?? item.id ?? item.model ?? item.provider ?? 'unknown');
    const label = String(item.display_name ?? item.displayName ?? item.display_name ?? item.name ?? item.id ?? id);
    const meta = kind === 'model'
      ? `${item.provider ?? item.modelProvider ?? ''}${item.contextWindow || item.context_window ? ` · ${item.contextWindow ?? item.context_window} ctx` : ''}`
      : kind === 'provider'
        ? `${item.kind ?? ''}${item.auth_type ? ` · ${item.auth_type}` : ''}`
        : `${item.mode ?? item.description ?? ''}`;
    const active = (kind === 'provider' && id === currentProvider) || (kind === 'model' && id === currentModel) || (kind === 'agent' && id === activeAgent);
    return `
      <button class="vv-soft-panel w-full p-3 text-left ${active ? 'border-vv-cyan bg-vv-cyan/10' : ''}" data-select-${kind}="${escapeHtml(id)}">
        <div class="flex items-center justify-between gap-2"><span class="truncate font-black">${escapeHtml(label)}</span>${active ? '<span class="vv-pill text-vv-cyan">active</span>' : ''}</div>
        <div class="mt-1 truncate text-xs text-vv-muted">${escapeHtml(meta)}</div>
      </button>`;
  }).join('');
  return `<h3 class="mb-2 font-black">${escapeHtml(title)}</h3><div class="space-y-2">${cards || '<p class="text-xs text-vv-muted">Not loaded.</p>'}</div>`;
}


function renderCapabilities(): string {
  const capabilities = state.capabilities ?? {};
  const nativeMethods = Array.isArray(capabilities.native_methods) ? capabilities.native_methods : [];
  const commandBacked = Array.isArray(capabilities.command_backed_methods) ? capabilities.command_backed_methods : [];
  const commandCount = Array.isArray(capabilities.commands) ? capabilities.commands.length : state.commands.length;
  return `
    <div class="space-y-4">
      <section class="vv-panel p-4">
        <div class="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h2 class="text-base font-black">Bridge parity map</h2>
            <p class="mt-2 max-w-3xl text-xs leading-5 text-vv-muted">The desktop app consumes Vegvisir through <code class="text-vv-cyan">vegvisir app-server</code>. Native methods carry typed UI data; command-backed methods provide named parity endpoints for the full harness command families.</p>
          </div>
          <button class="vv-action" data-bridge-method="bridge.capabilities">Refresh capabilities</button>
        </div>
        <div class="mt-4 grid gap-3 md:grid-cols-3">
          ${metricCard('Native methods', nativeMethods.length, 'typed bridge surface')}
          ${metricCard('Command-backed methods', commandBacked.length, 'slash-command parity')}
          ${metricCard('Slash commands', commandCount, 'registered TUI surface')}
        </div>
      </section>
      <div class="grid gap-4 lg:grid-cols-2">
        <section class="vv-panel p-4">
          <h3 class="mb-3 font-black">Native methods</h3>
          <div class="flex flex-wrap gap-2">${nativeMethods.map((method: string) => `<span class="vv-pill">${escapeHtml(method)}</span>`).join('') || '<span class="text-xs text-vv-muted">Not loaded.</span>'}</div>
        </section>
        <section class="vv-panel p-4">
          <h3 class="mb-3 font-black">Command-backed parity methods</h3>
          <div class="vv-scrollbar max-h-[38rem] space-y-2 overflow-auto pr-1">${commandBacked.map(renderBridgeMethodRow).join('') || '<p class="text-xs text-vv-muted">Not loaded.</p>'}</div>
        </section>
      </div>
      <section><h3 class="mb-2 font-black">Raw capability payload</h3>${renderPre(JSON.stringify(capabilities, null, 2))}</section>
    </div>`;
}

function metricCard(label: string, value: number | string, hint: string): string {
  return `<div class="vv-soft-panel p-4"><div class="text-2xl font-black text-vv-cyan">${escapeHtml(String(value))}</div><div class="mt-1 text-sm font-bold">${escapeHtml(label)}</div><div class="mt-1 text-xs text-vv-muted">${escapeHtml(hint)}</div></div>`;
}

function renderBridgeMethodRow(spec: any): string {
  const method = String(spec.method ?? '');
  const command = String(spec.command ?? '');
  const subcommand = spec.default_subcommand ? ` ${spec.default_subcommand}` : '';
  return `<button class="vv-soft-panel w-full p-3 text-left hover:border-vv-line2" data-bridge-method="${escapeHtml(method)}">
    <div class="font-mono text-xs font-black text-vv-cyan">${escapeHtml(method)}</div>
    <div class="mt-1 text-xs text-vv-muted">${escapeHtml(command + subcommand)}</div>
  </button>`;
}

function renderMemoryWorkbench(): string {
  const actions = [
    ['memory.status', 'Status', 'Active CMS-v2 user/project scope'],
    ['memory.recent', 'Recent', 'Recent project-scoped memories'],
    ['memory.usedThisTurn', 'Used this turn', 'Latest context artifact'],
    ['memory.writesThisSession', 'Writes this session', 'Memory writeback audit'],
    ['memory.context', 'Prepare context', 'ECM context for a message'],
    ['memory.recall', 'Recall', 'Retrieve CMS memories by query'],
    ['memory.searchChatGpt', 'Search ChatGPT archive', 'Explicit imported archive search'],
    ['memory.export', 'Export', 'Export memories through Vegvisir'],
  ];
  return renderMethodWorkbench('Memory / CMS / ECM', 'Memory operations remain owned by CMS-v2 and Vegvisir. The desktop app only sends structured bridge requests and displays returned evidence.', actions, 'memory.status', state.memory || methodOutputText('memory.status'));
}

function renderSkillsWorkbench(): string {
  const actions = [
    ['skills.status', 'Status', 'Skill registry and bundle status'],
    ['skills.compile', 'Compile', 'Compile local source/help into skill draft'],
    ['skills.route', 'Route', 'Route a task to matching skills'],
    ['skills.load', 'Load', 'Load skill body/context'],
    ['skills.eval', 'Eval', 'Run deterministic skill evals'],
    ['skills.forge', 'Forge', 'Prepare/refine skill bundle'],
    ['skills.patch', 'Patch', 'Patch curated skills'],
    ['skills.curate', 'Curate', 'Curate registry candidates'],
    ['skills.detect', 'Detect', 'Detect skill opportunities'],
    ['skills.trace', 'Trace', 'Inspect skill routing traces'],
    ['skills.promote', 'Promote', 'Promote bundle to registry'],
    ['skills.archive', 'Archive', 'Archive bundle'],
  ];
  return renderMethodWorkbench('Skills / Skiller / LSL', 'These entry points expose the skill system without reimplementing Skiller in the GUI.', actions, 'skills.status', methodOutputText('skills.status'));
}

function renderIntegrationsWorkbench(): string {
  const actions = [
    ['mcp.status', 'MCP status', 'Configured MCP servers'],
    ['mcp.tools', 'MCP tools', 'Available MCP tools'],
    ['mcp.authMap', 'Auth map', 'MCP/HBSE auth mapping'],
    ['mcp.reload', 'Reload MCP', 'Reload configured servers'],
    ['hbse.status', 'HBSE status', 'Secret-ref broker state'],
    ['hbse.services', 'HBSE services', 'Service-ref registry'],
    ['hbse.usageThisSession', 'HBSE session usage', 'Secret-ref usage audit'],
    ['subagents.list', 'Subagents', 'Durable worker board'],
    ['subagents.timeline', 'Timeline', 'Subagent lifecycle events'],
    ['subagents.policy', 'Policy', 'Subagent concurrency policy'],
    ['agents.templates', 'Agent templates', 'Reusable agent templates'],
    ['agents.show', 'Agent show', 'Inspect active/specified agent'],
  ];
  return renderMethodWorkbench('Integrations', 'MCP, HBSE, agents, and subagents stay behind Vegvisir policy. Do not paste plaintext secrets into any field.', actions, 'mcp.status', [methodOutputText('mcp.status'), methodOutputText('hbse.status'), methodOutputText('subagents.list')].filter(Boolean).join('\n\n'));
}

function renderEvidenceWorkbench(): string {
  const actions = [
    ['verify.run', 'Verify', 'Run readiness checks'],
    ['eval.run', 'Eval', 'Run deterministic eval suite'],
    ['trace.list', 'Trace', 'Recent harness trace events'],
    ['work.list', 'Work log', 'Tool/activity timeline'],
    ['runs.list', 'Runs', 'Run artifact bundles'],
    ['runs.show', 'Show run', 'Inspect latest/specified run'],
    ['runs.diff', 'Run diff', 'Captured run diff'],
    ['runs.replayPlan', 'Replay plan', 'Manual replay checklist'],
  ];
  return renderMethodWorkbench('Evidence / verification', 'Use these controls before trusting or packaging changes. Outputs are generated by Vegvisir commands and artifacts.', actions, 'runs.list', [methodOutputText('runs.list'), methodOutputText('trace.list'), methodOutputText('verify.run')].filter(Boolean).join('\n\n'));
}

function renderMethodWorkbench(title: string, description: string, actions: string[][], primaryMethod: string, output: string): string {
  const selected = state.selectedBridgeMethod || primaryMethod;
  const selectedOutput = methodOutputText(selected);
  return `
    <div class="space-y-4">
      <section class="vv-panel p-4">
        <div class="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h2 class="text-base font-black">${escapeHtml(title)}</h2>
            <p class="mt-2 max-w-3xl text-xs leading-5 text-vv-muted">${escapeHtml(description)}</p>
          </div>
          <button class="vv-action" data-bridge-method="${escapeHtml(primaryMethod)}">Refresh</button>
        </div>
        ${renderBridgeMethodParameterForm(selected)}
        <div class="mt-4 grid gap-2 md:grid-cols-2 xl:grid-cols-3">
          ${actions.map(([method, label, hint]) => `<button class="vv-soft-panel p-3 text-left hover:border-vv-line2 ${selected === method ? 'border-vv-cyan bg-vv-cyan/10' : ''}" data-bridge-method="${escapeHtml(method)}">
            <div class="font-black">${escapeHtml(label)}</div>
            <div class="mt-1 font-mono text-[0.68rem] text-vv-cyan">${escapeHtml(method)}</div>
            <div class="mt-1 text-xs text-vv-muted">${escapeHtml(hint)}</div>
          </button>`).join('')}
        </div>
      </section>
      <section><h3 class="mb-2 font-black">Latest output${selected ? ` · ${escapeHtml(selected)}` : ''}</h3>${renderPre(selectedOutput || output || 'No output loaded yet. Run an action above.')}</section>
    </div>`;
}

function renderBridgeMethodParameterForm(selectedMethod: string): string {
  const draft = state.bridgeDraft;
  return `
    <div class="mt-4 rounded-2xl border border-vv-line bg-black/16 p-3">
      <div class="mb-2 flex flex-wrap items-center justify-between gap-2">
        <div>
          <h3 class="text-sm font-black">Parameterized method call</h3>
          <p class="mt-1 text-xs text-vv-muted">Use raw for exact slash-command arguments, or fill common fields used by recall, runs, agents, subagents, skills, and memory methods.</p>
        </div>
        <button class="vv-action vv-action-primary" data-run-selected-bridge-method="${escapeHtml(selectedMethod)}">Run selected</button>
      </div>
      <div class="grid gap-2 md:grid-cols-2 xl:grid-cols-4">
        ${bridgeDraftField('raw', 'Raw args', draft.raw, 'e.g. recent --global')}
        ${bridgeDraftField('query', 'Query', draft.query, 'search or context query')}
        ${bridgeDraftField('id', 'ID', draft.id, 'run/task/memory id')}
        ${bridgeDraftField('name', 'Name', draft.name, 'agent/project/title')}
        ${bridgeDraftField('path', 'Path', draft.path, 'file or bundle path')}
        ${bridgeDraftField('value', 'Value', draft.value, 'setting/new value')}
        ${bridgeDraftField('target', 'Target', draft.target, 'verify/eval target')}
        ${bridgeDraftField('scope', 'Scope', draft.scope, 'all/runtime/global')}
        ${bridgeDraftField('limit', 'Limit', draft.limit, 'numeric limit')}
        <label class="flex items-center gap-2 rounded-xl border border-vv-line bg-black/20 px-3 py-2 text-xs text-vv-muted"><input type="checkbox" data-bridge-param="global" ${draft.global ? 'checked' : ''} /> Global</label>
      </div>
    </div>`;
}

function bridgeDraftField(name: keyof Omit<BridgeMethodDraft, 'global'>, label: string, value: string, placeholder: string): string {
  return `<label class="grid gap-1"><span class="text-[0.68rem] text-vv-muted">${escapeHtml(label)}</span><input class="vv-focus rounded-xl border border-vv-line bg-black/20 px-3 py-2 text-xs text-vv-text placeholder:text-vv-dim" data-bridge-param="${escapeHtml(name)}" value="${escapeHtml(value)}" placeholder="${escapeHtml(placeholder)}" /></label>`;
}

function methodOutputText(method: string): string {
  const payload = state.methodOutputs[method];
  if (!payload) return '';
  return payload.output ?? JSON.stringify(payload, null, 2);
}

function renderCommands(): string {
  const commands = Array.isArray(state.commands) ? state.commands : [];
  const groups = commandGroups(commands);
  return `
    <div class="space-y-4">
      <div class="vv-panel p-4">
        <h2 class="text-base font-black">Full Vegvisir command surface</h2>
        <p class="mt-2 text-xs leading-5 text-vv-muted">Every registered Vegvisir slash command is discoverable here and can be invoked through <code class="text-vv-cyan">command.invoke</code>. Structured app panels are convenience wrappers, not a separate runtime.</p>
        <div class="mt-3 flex gap-2">
          <input id="command-palette-input" class="vv-focus min-w-0 flex-1 rounded-xl border border-vv-line bg-black/20 px-3 py-2 text-xs text-vv-text placeholder:text-vv-dim" placeholder="/subagents list, /mcp status, /skills status, /verify runtime..." />
          <button id="run-palette-command" class="vv-action">Invoke</button>
        </div>
      </div>
      ${Object.entries(groups).map(([group, items]) => `
        <section class="vv-panel p-4">
          <h3 class="mb-3 text-sm font-black uppercase tracking-[0.18em] text-vv-muted">${escapeHtml(group)}</h3>
          <div class="grid gap-2 md:grid-cols-2 xl:grid-cols-3">
            ${(items as any[]).map(renderCommandCard).join('')}
          </div>
        </section>`).join('')}
    </div>`;
}

function commandGroups(commands: any[]): Record<string, any[]> {
  const groups: Record<string, any[]> = {
    Sessions: [], Runtime: [], Providers: [], Memory: [], Skills: [], Integrations: [], Evidence: [], Other: [],
  };
  for (const command of commands) {
    const name = String(command.name ?? '');
    const bucket = ['/new','/sessions','/load','/workspace','/projects','/save','/branch','/fork','/title'].includes(name) ? 'Sessions'
      : ['/tools','/tool-limit','/approvals','/auto','/autonomy','/cancel','/turn-repair','/recover','/status'].includes(name) ? 'Runtime'
      : ['/models','/model','/provider','/providers','/auth','/effort','/fast','/model-request'].includes(name) ? 'Providers'
      : ['/memory','/recall','/remember','/context','/summary','/handoff'].includes(name) ? 'Memory'
      : ['/skills'].includes(name) ? 'Skills'
      : ['/mcp','/hbse','/subagents','/agent','/agents','/speech','/tts','/config','/profile','/ka'].includes(name) ? 'Integrations'
      : ['/diff','/runs','/verify','/eval','/trace','/work','/history','/system','/system-prompt'].includes(name) ? 'Evidence'
      : 'Other';
    groups[bucket].push(command);
  }
  return Object.fromEntries(Object.entries(groups).filter(([, items]) => items.length));
}

function renderCommandCard(command: any): string {
  const name = String(command.name ?? '');
  return `
    <article class="vv-soft-panel p-3">
      <div class="flex items-center justify-between gap-2"><h4 class="font-mono text-sm font-black text-vv-cyan">${escapeHtml(name)}</h4><button class="vv-action px-2 py-1 text-[0.65rem]" data-command-run="${escapeHtml(name)}">Run</button></div>
      <p class="mt-2 text-xs leading-5 text-vv-muted">${escapeHtml(command.description ?? '')}</p>
      <pre class="vv-code mt-2 truncate text-[0.68rem] text-vv-dim">${escapeHtml(command.usage ?? name)}</pre>
    </article>`;
}

function renderRuntime(): string {
  const runtime = state.runtimeStatus ?? {};
  return `
    <div class="space-y-4">
      <section class="vv-panel p-4">
        <h2 class="mb-2 text-base font-black">Harness runtime and policy</h2>
        <p class="mb-3 text-xs leading-5 text-vv-muted">These controls call structured bridge methods or Vegvisir slash commands; approvals, command allow-lists, HBSE, CMS, USRL, and dangerous-bypass policy remain enforced by the harness.</p>
        <div class="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
          <button class="vv-action" data-fast="true">Fast on</button>
          <button class="vv-action" data-fast="false">Fast off</button>
          <select id="effort-select" class="vv-focus rounded-xl border border-vv-line bg-black/30 px-3 py-2 text-sm text-vv-text">
            <option value="default">effort: default</option><option value="minimal">minimal</option><option value="low">low</option><option value="medium">medium</option><option value="high">high</option>
          </select>
          <div class="flex gap-2"><input id="tool-limit-input" class="vv-focus min-w-0 rounded-xl border border-vv-line bg-black/30 px-3 py-2 text-sm text-vv-text" placeholder="tool rounds/default" /><button id="set-tool-limit" class="vv-action">Set</button></div>
        </div>
      </section>
      <div class="grid gap-4 lg:grid-cols-2"><section><h3 class="mb-2 font-black">Status</h3>${renderPre(JSON.stringify(runtime.session ?? state.session ?? {}, null, 2))}</section><section><h3 class="mb-2 font-black">Policy output</h3>${renderPre(`${runtime.status_output ?? ''}

${runtime.tools_output ?? ''}`.trim() || 'No runtime status loaded.')}</section></div>
    </div>`;
}

function renderOpenAiBridge(): string {
  const info = state.openaiCompat ?? {};
  const hbse = state.hbseOnboarding ?? {};
  return `
    <div class="space-y-4">
      <section class="vv-panel p-4">
        <h2 class="text-base font-black">OpenAI-compatible Vegvisir bridge</h2>
        <p class="mt-2 text-xs leading-5 text-vv-muted">Use this only to point OpenAI-compatible clients at Vegvisir. Provider access still flows through Vegvisir/HBSE; the desktop app does not request or expose API keys.</p>
        <div class="mt-3 grid gap-3 md:grid-cols-2">
          <div><div class="text-xs text-vv-muted">Base URL</div><pre class="vv-code vv-panel mt-1 p-3">${escapeHtml(info.base_url ?? 'Not loaded')}</pre></div>
          <div><div class="text-xs text-vv-muted">Launch command</div><pre class="vv-code vv-panel mt-1 p-3 whitespace-pre-wrap">${escapeHtml(info.launch_command ?? 'Not loaded')}</pre></div>
        </div>
      </section>
      <div class="grid gap-4 lg:grid-cols-2">
        <section><h3 class="mb-2 font-black">OpenAI compat metadata</h3>${renderPre(JSON.stringify(info, null, 2))}</section>
        <section><h3 class="mb-2 font-black">HBSE onboarding metadata</h3>${renderPre(JSON.stringify(hbse, null, 2))}</section>
      </div>
    </div>`;
}

function renderSystem(): string {
  return `<div><button id="refresh-system" class="vv-action mb-3">Refresh system prompt</button>${renderPre(state.systemPrompt || 'No system prompt loaded.')}</div>`;
}

function renderSettings(): string {
  return `
    <form class="vv-panel grid max-w-3xl gap-3 p-4" id="settings-form">
      ${field('vegvisirBinary', 'Vegvisir binary', state.settings.vegvisirBinary ?? 'vegvisir')}
      ${field('workspace', 'Workspace', state.settings.workspace ?? '')}
      ${field('provider', 'Provider', state.settings.provider ?? '')}
      ${field('model', 'Model', state.settings.model ?? '')}
      ${field('agent', 'Agent', state.settings.agent ?? '')}
      <label class="flex items-center gap-3 text-sm text-vv-muted"><input type="checkbox" name="autoStart" ${state.settings.autoStart === false ? '' : 'checked'} /> Auto-start bridge when the desktop app opens</label>
      <label class="flex items-center gap-3 text-sm text-vv-muted"><input type="checkbox" name="dangerousBypass" ${state.settings.dangerousBypass ? 'checked' : ''} /> Dangerous bypass at startup</label>
      <p class="text-xs leading-5 text-vv-muted">Packaged AppImages may not inherit your shell PATH. If bridge start fails, set the Vegvisir binary to an absolute path such as <code class="text-vv-cyan">/home/malice/.local/bin/vegvisir</code>. The backend also searches resource-adjacent <code class="text-vv-cyan">resources/bin</code> paths and <code class="text-vv-cyan">VEGVISIR_DESKTOP_RESOURCE_DIR</code> for future bundled binaries.</p>
      <p class="text-xs leading-5 text-vv-muted">Desktop does not bypass Vegvisir. It spawns <code class="text-vv-cyan">vegvisir app-server</code> so providers, HBSE, CMS, tools, approvals, and policy remain owned by the harness.</p>
      <button class="vv-action vv-action-primary w-fit" type="submit">Save settings</button>
    </form>
  `;
}

function field(name: string, label: string, value: string): string {
  return `<label class="grid gap-1.5"><span class="text-xs text-vv-muted">${escapeHtml(label)}</span><input class="vv-focus rounded-xl border border-vv-line bg-black/20 px-3 py-2 text-sm text-vv-text" name="${name}" value="${escapeHtml(value)}" /></label>`;
}

function renderPre(value: string): string {
  return `<pre class="vv-code vv-panel vv-scrollbar max-h-[72vh] overflow-auto whitespace-pre-wrap break-words p-4">${escapeHtml(value)}</pre>`;
}

function activeTitle(): string {
  const label = panels.find((panel) => panel.id === state.activePanel)?.label ?? 'Workbench';
  if (state.activePanel === 'chat') return state.busy ? 'Vegvisir is working…' : 'Ask Vegvisir to work';
  return label;
}

function projectName(): string {
  const workspace = state.session?.workspace ?? state.settings.workspace ?? '';
  const trimmed = String(workspace).replace(/\/$/, '');
  if (!trimmed) return 'local';
  return trimmed.split('/').filter(Boolean).pop() ?? trimmed;
}

function bindEvents(): void {
  document.querySelector('#start-stop')?.addEventListener('click', () => state.bridgeRunning ? void stopBridge() : void startBridge());
  document.querySelector('#restart-bridge')?.addEventListener('click', () => void restartBridge());
  document.querySelector('#restart-bridge-from-error')?.addEventListener('click', () => void restartBridge());
  document.querySelector('#refresh-all')?.addEventListener('click', () => void refreshEverything());
  document.querySelectorAll<HTMLButtonElement>('[data-panel]').forEach((button) => button.addEventListener('click', () => setPanel(button.dataset.panel ?? 'chat')));
  document.querySelector('#send-turn')?.addEventListener('click', () => void sendTurn());
  document.querySelector('#turn-input')?.addEventListener('keydown', (event) => {
    const key = event as KeyboardEvent;
    if (key.key === 'Enter' && !key.shiftKey) {
      key.preventDefault();
      void sendTurn();
    }
  });
  document.querySelector('#refresh-sessions')?.addEventListener('click', () => void send('session.list', {}, 'sessions'));
  document.querySelectorAll<HTMLButtonElement>('[data-session-load]').forEach((button) => button.addEventListener('click', () => void loadSession(button.dataset.sessionLoad ?? '')));
  document.querySelector('#refresh-system')?.addEventListener('click', () => void send('system.prompt', {}, 'system'));
  document.querySelectorAll<HTMLButtonElement>('[data-approval]').forEach((button) => button.addEventListener('click', () => void approve(button.dataset.approval ?? '', button.dataset.method ?? 'approvals.deny')));
  document.querySelectorAll<HTMLButtonElement>('[data-select-provider]').forEach((button) => button.addEventListener('click', () => void selectProvider(button.dataset.selectProvider ?? '')));
  document.querySelectorAll<HTMLButtonElement>('[data-select-model]').forEach((button) => button.addEventListener('click', () => void selectModel(button.dataset.selectModel ?? '')));
  document.querySelectorAll<HTMLButtonElement>('[data-select-agent]').forEach((button) => button.addEventListener('click', () => void selectAgent(button.dataset.selectAgent ?? '')));
  document.querySelectorAll<HTMLButtonElement>('[data-command-run]').forEach((button) => button.addEventListener('click', () => void runCommandValue(button.dataset.commandRun ?? '')));
  document.querySelectorAll<HTMLInputElement>('[data-bridge-param]').forEach((input) => input.addEventListener('input', updateBridgeDraftFromForm));
  document.querySelectorAll<HTMLInputElement>('[data-bridge-param]').forEach((input) => input.addEventListener('change', updateBridgeDraftFromForm));
  document.querySelectorAll<HTMLButtonElement>('[data-bridge-method]').forEach((button) => button.addEventListener('click', () => void callBridgeMethodFromWorkbench(button.dataset.bridgeMethod ?? '')));
  document.querySelectorAll<HTMLButtonElement>('[data-run-selected-bridge-method]').forEach((button) => button.addEventListener('click', () => void callBridgeMethodFromWorkbench(button.dataset.runSelectedBridgeMethod || state.selectedBridgeMethod || '')));
  document.querySelector('#run-palette-command')?.addEventListener('click', () => {
    const input = document.querySelector<HTMLInputElement>('#command-palette-input');
    void runCommandValue(input?.value.trim() ?? '');
  });
  document.querySelectorAll<HTMLButtonElement>('[data-fast]').forEach((button) => button.addEventListener('click', () => void setFastMode(button.dataset.fast === 'true')));
  document.querySelector('#effort-select')?.addEventListener('change', (event) => void setEffort((event.currentTarget as HTMLSelectElement).value));
  document.querySelector('#set-tool-limit')?.addEventListener('click', () => {
    const input = document.querySelector<HTMLInputElement>('#tool-limit-input');
    void setToolLimit(input?.value.trim() || 'default');
  });
  document.querySelector('#settings-form')?.addEventListener('submit', (event) => {
    event.preventDefault();
    const form = new FormData(event.currentTarget as HTMLFormElement);
    state.settings = {
      vegvisirBinary: String(form.get('vegvisirBinary') ?? 'vegvisir'),
      workspace: String(form.get('workspace') ?? ''),
      provider: String(form.get('provider') ?? ''),
      model: String(form.get('model') ?? ''),
      agent: String(form.get('agent') ?? ''),
      autoStart: form.get('autoStart') === 'on',
      dangerousBypass: form.get('dangerousBypass') === 'on',
    };
    saveSettings();
    render();
  });
}

function updateBridgeDraftFromForm(event: Event): void {
  const input = event.currentTarget as HTMLInputElement;
  const key = input.dataset.bridgeParam as keyof BridgeMethodDraft | undefined;
  if (!key) return;
  if (key === 'global') state.bridgeDraft.global = input.checked;
  else state.bridgeDraft[key] = input.value;
}

function escapeHtml(value: string): string {
  return String(value)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#039;');
}

async function bootstrap(): Promise<void> {
  render();
  await refreshStatus();
  if (!state.bridgeRunning && state.settings.autoStart !== false) {
    state.autoStartAttempted = true;
    await startBridge();
  } else {
    render();
  }
}

void bootstrap();
setInterval(() => void pollBridge(), 350);

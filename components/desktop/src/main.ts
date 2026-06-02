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

type PanelId = 'chat' | 'work' | 'approvals' | 'tools' | 'providers' | 'diff' | 'memory' | 'system' | 'settings';

const appElement = document.querySelector<HTMLDivElement>('#app');
if (!appElement) throw new Error('missing #app root');
const app = appElement;

const panels: Array<{ id: PanelId; label: string; icon: string; hint: string }> = [
  { id: 'chat', label: 'Chat', icon: '✦', hint: 'Active agent transcript' },
  { id: 'work', label: 'Work log', icon: '◌', hint: 'Bridge and tool events' },
  { id: 'approvals', label: 'Approvals', icon: '◇', hint: 'Risk gates' },
  { id: 'tools', label: 'Tools', icon: '⌘', hint: 'Harness capabilities' },
  { id: 'providers', label: 'Providers', icon: '⬡', hint: 'Models and agents' },
  { id: 'diff', label: 'Diff', icon: '±', hint: 'Workspace changes' },
  { id: 'memory', label: 'Memory', icon: '◎', hint: 'CMS/ECM state' },
  { id: 'system', label: 'System', icon: '◈', hint: 'Prompt and policy' },
  { id: 'settings', label: 'Settings', icon: '⚙', hint: 'Bridge launch config' },
];

const state = {
  requestCounter: 0,
  bridgeRunning: false,
  bridgePid: null as number | null,
  autoStartAttempted: false,
  session: null as any,
  events: [] as BridgeEvent[],
  messages: [] as Message[],
  pendingAssistant: '',
  approvals: [] as any[],
  tools: [] as any[],
  providers: [] as any[],
  models: [] as any[],
  agents: [] as any[],
  diff: '',
  memory: '',
  systemPrompt: '',
  activePanel: 'chat' as PanelId,
  busy: false,
  error: '',
  settings: loadSettings(),
};

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
    const status = await invoke<{ running: boolean; pid?: number }>('bridge_start', { request: compactSettings() });
    state.bridgeRunning = status.running;
    state.bridgePid = status.pid ?? null;
    render();
    await send('initialize', {}, 'initialize');
    await refreshEverything();
  } catch (error) {
    state.bridgeRunning = false;
    state.bridgePid = null;
    state.error = String(error);
    state.activePanel = 'settings';
    render();
  }
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
  await invoke('bridge_stop');
  state.bridgeRunning = false;
  state.bridgePid = null;
  state.session = null;
  state.events = [];
  state.messages = [];
  state.pendingAssistant = '';
  render();
}

async function refreshStatus(): Promise<void> {
  try {
    const status = await invoke<{ running: boolean; pid?: number }>('bridge_status');
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
    send('approvals.list', {}, 'approvals'),
    send('tools.list', {}, 'tools'),
    send('providers.list', {}, 'providers'),
    send('models.list', {}, 'models'),
    send('agents.list', {}, 'agents'),
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

  switch (event.type) {
    case 'desktop.bridge.spawned':
      break;
    case 'desktop.bridge.exited':
      state.bridgeRunning = false;
      state.bridgePid = null;
      state.error = `Bridge exited: ${event.payload?.status ?? 'unknown status'}`;
      break;
    case 'desktop.bridge.error':
      state.bridgeRunning = false;
      state.bridgePid = null;
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
  state.messages.push({ role: 'user', content });
  state.error = '';
  render();
  await send('turn.send', { content }, 'turn');
}

async function runSlashCommand(): Promise<void> {
  const input = document.querySelector<HTMLInputElement>('#command-input');
  const command = input?.value.trim() ?? '';
  if (!command || !state.bridgeRunning) return;
  input!.value = '';
  await send('command.run', { command }, 'command');
}

function setPanel(panel: string): void {
  state.activePanel = panel as PanelId;
  if (panel === 'diff') void send('diff.current', {}, 'diff');
  if (panel === 'memory') void send('memory.status', {}, 'memory');
  if (panel === 'system') void send('system.prompt', {}, 'system');
  render();
}

async function approve(id: string, method: string): Promise<void> {
  await send(method, { id }, 'approval');
  await send('approvals.list', {}, 'approvals');
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
        <button class="vv-action" id="refresh-all">Refresh</button>
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
  return `<div class="mx-auto mt-3 max-w-5xl rounded-xl border border-vv-red/45 bg-vv-red/10 p-3 text-red-100 shadow-danger"><strong>Bridge problem</strong><pre class="vv-code mt-2 whitespace-pre-wrap">${escapeHtml(state.error)}</pre></div>`;
}

function renderPanel(): string {
  if (state.activePanel === 'chat') return `<div class="min-h-0 flex-1 overflow-hidden">${renderChat()}</div>`;
  return `<div class="vv-scrollbar min-h-0 flex-1 overflow-auto px-4 py-3"><div class="mx-auto max-w-5xl">${renderNonChatPanel()}</div></div>`;
}

function renderNonChatPanel(): string {
  switch (state.activePanel) {
    case 'work': return renderWork();
    case 'approvals': return renderApprovals();
    case 'tools': return renderTools();
    case 'providers': return renderProviders();
    case 'diff': return renderPre(state.diff || 'No diff loaded.');
    case 'memory': return renderPre(state.memory || 'No memory status loaded.');
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
        <textarea id="turn-input" class="vv-focus vv-scrollbar h-16 max-h-16 min-h-16 w-full resize-none rounded-xl border border-transparent bg-transparent px-2.5 py-1.5 text-[0.92rem] leading-6 text-vv-text placeholder:text-vv-dim" placeholder="Ask Vegvisir anything, @tag files/folders, or use /command" ${state.bridgeRunning ? '' : 'disabled'}></textarea>
        <div class="mt-1.5 flex items-center justify-between gap-2 border-t border-vv-line pt-2">
          <div class="flex min-w-0 items-center gap-1.5 overflow-hidden text-xs text-vv-muted">
            <span class="vv-pill">${escapeHtml(state.settings.model || 'model default')}</span>
            <span class="vv-pill">${state.busy ? 'High activity' : 'Ready'}</span>
            <span class="vv-pill">Chat</span>
            <span class="vv-pill">${state.settings.dangerousBypass ? 'Bypass startup' : 'Policy gated'}</span>
          </div>
          <button id="send-turn" class="vv-focus grid h-9 w-9 shrink-0 place-items-center rounded-full ${state.busy ? 'bg-vv-red' : 'bg-vv-pink'} text-lg font-black text-white shadow-[0_0_28px_rgba(255,46,126,0.28)]" ${state.bridgeRunning && !state.busy ? '' : 'disabled'}>${state.busy ? '■' : '➤'}</button>
        </div>
      </div>
      <div class="mt-2 flex gap-2">
        <input id="command-input" class="vv-focus min-w-0 flex-1 rounded-xl border border-vv-line bg-black/20 px-3 py-2 text-xs text-vv-text placeholder:text-vv-dim" placeholder="Run slash command, e.g. /tools or /diff" ${state.bridgeRunning ? '' : 'disabled'} />
        <button id="run-command" class="vv-action" ${state.bridgeRunning ? '' : 'disabled'}>Run command</button>
      </div>
    </div>
  `;
}

function renderMessage(message: Message): string {
  const role = message.role ?? 'message';
  const isUser = role === 'user';
  const isTool = role.includes('tool') || role.includes('event');
  const cardClass = isUser ? 'ml-auto max-w-3xl bg-white/[0.07]' : isTool ? 'max-w-4xl border-vv-line bg-black/18 opacity-75' : 'max-w-4xl bg-white/[0.035]';
  return `
    <article class="vv-soft-panel ${cardClass}">
      <header class="flex items-center gap-3 border-b border-vv-line px-4 py-2 text-[0.68rem] uppercase tracking-[0.22em] text-vv-muted"><span class="h-2 w-2 rounded-full ${isUser ? 'bg-vv-green' : 'bg-vv-cyan'}"></span>${escapeHtml(role)}</header>
      <pre class="vv-code whitespace-pre-wrap break-words px-4 py-3">${escapeHtml(message.content ?? message.text ?? '')}</pre>
    </article>
  `;
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
  return `<div class="grid gap-4 lg:grid-cols-2"><section><h2 class="mb-3 text-base font-black">Providers</h2>${renderPre(JSON.stringify(state.providers, null, 2))}</section><section><h2 class="mb-3 text-base font-black">Models</h2>${renderPre(JSON.stringify(state.models, null, 2))}<h2 class="mb-3 mt-5 text-base font-black">Agents</h2>${renderPre(JSON.stringify(state.agents, null, 2))}</section></div>`;
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
      <p class="text-xs leading-5 text-vv-muted">Packaged AppImages may not inherit your shell PATH. If bridge start fails, set the Vegvisir binary to an absolute path such as <code class="text-vv-cyan">/home/malice/.local/bin/vegvisir</code>.</p>
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
  document.querySelector('#refresh-all')?.addEventListener('click', () => void refreshEverything());
  document.querySelectorAll<HTMLButtonElement>('[data-panel]').forEach((button) => button.addEventListener('click', () => setPanel(button.dataset.panel ?? 'chat')));
  document.querySelector('#send-turn')?.addEventListener('click', () => void sendTurn());
  document.querySelector('#run-command')?.addEventListener('click', () => void runSlashCommand());
  document.querySelector('#refresh-system')?.addEventListener('click', () => void send('system.prompt', {}, 'system'));
  document.querySelectorAll<HTMLButtonElement>('[data-approval]').forEach((button) => button.addEventListener('click', () => void approve(button.dataset.approval ?? '', button.dataset.method ?? 'approvals.deny')));
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

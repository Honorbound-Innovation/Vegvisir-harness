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
};

const app = document.querySelector<HTMLDivElement>('#app');
if (!app) {
  throw new Error('missing #app root');
}

const state = {
  requestCounter: 0,
  bridgeRunning: false,
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
  activePanel: 'chat',
  busy: false,
  error: '',
  settings: loadSettings(),
};

function loadSettings(): StartBridgeRequest {
  const raw = localStorage.getItem('vegvisir.desktop.settings');
  if (!raw) {
    return {
      vegvisirBinary: 'vegvisir',
      workspace: '',
      provider: '',
      model: '',
      agent: '',
      dangerousBypass: false,
    };
  }
  try {
    return JSON.parse(raw);
  } catch {
    return { vegvisirBinary: 'vegvisir' };
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
    render();
    await send('initialize', {}, 'initialize');
    await refreshEverything();
  } catch (error) {
    state.error = String(error);
    render();
  }
}

function compactSettings(): StartBridgeRequest {
  const result: StartBridgeRequest = {};
  for (const [key, value] of Object.entries(state.settings)) {
    if (typeof value === 'string' && value.trim() !== '') {
      (result as any)[key] = value.trim();
    } else if (typeof value === 'boolean') {
      (result as any)[key] = value;
    }
  }
  return result;
}

async function stopBridge(): Promise<void> {
  await invoke('bridge_stop');
  state.bridgeRunning = false;
  state.session = null;
  state.events = [];
  state.messages = [];
  state.pendingAssistant = '';
  render();
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
  if (!state.bridgeRunning) return;
  try {
    const lines = await invoke<string[]>('bridge_poll');
    if (!lines.length) return;
    for (const line of lines) {
      try {
        handleEvent(JSON.parse(line));
      } catch {
        handleEvent({ type: 'bridge.raw', payload: { line } });
      }
    }
    render();
  } catch (error) {
    state.error = String(error);
    render();
  }
}

function handleEvent(event: BridgeEvent): void {
  state.events.push(event);
  if (state.events.length > 600) {
    state.events.splice(0, state.events.length - 600);
  }

  switch (event.type) {
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
      if (state.pendingAssistant.trim()) {
        state.messages.push({ role: 'assistant', content: state.pendingAssistant });
      }
      state.pendingAssistant = '';
      void send('session.messages', {}, 'messages');
      void send('session.status', {}, 'status');
      break;
    case 'turn.failed':
      state.busy = false;
      state.error = event.payload?.message ?? 'turn failed';
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
      state.error = event.payload?.message ?? JSON.stringify(event.payload);
      break;
  }
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
  state.activePanel = panel;
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
    <div class="shell">
      <aside class="sidebar">
        <div class="brand"><span class="brand-mark">V</span><div><strong>Vegvisir</strong><small>Desktop</small></div></div>
        <button class="primary" id="start-stop">${state.bridgeRunning ? 'Stop bridge' : 'Start bridge'}</button>
        <nav>
          ${navButton('chat', 'Chat')}
          ${navButton('work', 'Work log')}
          ${navButton('approvals', `Approvals ${state.approvals.length ? `(${state.approvals.length})` : ''}`)}
          ${navButton('tools', 'Tools')}
          ${navButton('providers', 'Providers')}
          ${navButton('diff', 'Diff')}
          ${navButton('memory', 'Memory')}
          ${navButton('system', 'System')}
          ${navButton('settings', 'Settings')}
        </nav>
      </aside>
      <main class="main">
        <header class="topbar">
          <div>${sessionSummary()}</div>
          <div class="status ${state.bridgeRunning ? 'ok' : ''}">${state.bridgeRunning ? 'bridge online' : 'bridge offline'}</div>
        </header>
        ${state.error ? `<div class="error">${escapeHtml(state.error)}</div>` : ''}
        <section class="content">${renderPanel()}</section>
      </main>
    </div>
  `;
  bindEvents();
}

function navButton(panel: string, label: string): string {
  return `<button class="nav ${state.activePanel === panel ? 'active' : ''}" data-panel="${panel}">${escapeHtml(label)}</button>`;
}

function sessionSummary(): string {
  const session = state.session ?? {};
  return `
    <div class="summary">
      <strong>${escapeHtml(session.workspace ?? state.settings.workspace ?? 'No workspace selected')}</strong>
      <span>provider ${escapeHtml(session.provider ?? state.settings.provider ?? 'default')}</span>
      <span>model ${escapeHtml(session.model ?? state.settings.model ?? 'default')}</span>
      <span>tools ${escapeHtml(String(session.tools_enabled ?? '—'))}</span>
      <span>ctx ${escapeHtml(String(session.tokens_used ?? '—'))}</span>
    </div>
  `;
}

function renderPanel(): string {
  switch (state.activePanel) {
    case 'chat': return renderChat();
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
    <div class="chat-layout">
      <div class="messages">
        ${messages.map(renderMessage).join('') || '<p class="muted">Start the bridge, then send a task. The desktop app talks to the same Vegvisir runtime as the TUI.</p>'}
      </div>
      <div class="composer">
        <textarea id="turn-input" placeholder="Ask Vegvisir to inspect, build, fix, document, verify..." ${state.bridgeRunning ? '' : 'disabled'}></textarea>
        <button id="send-turn" class="primary" ${state.bridgeRunning && !state.busy ? '' : 'disabled'}>${state.busy ? 'Working…' : 'Send'}</button>
      </div>
      <div class="command-row">
        <input id="command-input" placeholder="Run slash command, e.g. /tools or /diff" ${state.bridgeRunning ? '' : 'disabled'} />
        <button id="run-command" ${state.bridgeRunning ? '' : 'disabled'}>Run command</button>
      </div>
    </div>
  `;
}

function renderMessage(message: Message): string {
  return `<article class="message ${escapeHtml(message.role ?? 'message')}"><header>${escapeHtml(message.role ?? 'message')}</header><pre>${escapeHtml(message.content ?? message.text ?? '')}</pre></article>`;
}

function renderWork(): string {
  return `<div class="event-list">${state.events.slice().reverse().map((event) => `
    <article class="event"><strong>${escapeHtml(event.type)}</strong><pre>${escapeHtml(JSON.stringify(event.payload ?? {}, null, 2))}</pre></article>
  `).join('')}</div>`;
}

function renderApprovals(): string {
  if (!state.approvals.length) return '<p class="muted">No pending approvals. The beast is behaving.</p>';
  return `<div class="cards">${state.approvals.map((approval) => `
    <article class="card danger">
      <h3>${escapeHtml(approval.tool_name ?? approval.toolName ?? 'approval')}</h3>
      <p>${escapeHtml(approval.reason ?? approval.risk_label ?? 'Risky action requires approval.')}</p>
      <pre>${escapeHtml(JSON.stringify(approval.args ?? {}, null, 2))}</pre>
      <div class="actions">
        <button data-approval="${escapeHtml(approval.id)}" data-method="approvals.approveOnce">Approve once</button>
        <button data-approval="${escapeHtml(approval.id)}" data-method="approvals.approveSession">Approve session</button>
        <button data-approval="${escapeHtml(approval.id)}" data-method="approvals.deny">Deny</button>
      </div>
    </article>`).join('')}</div>`;
}

function renderTools(): string {
  return `<div class="cards">${state.tools.map((tool) => `<article class="card"><h3>${escapeHtml(tool.name)}</h3><p>${escapeHtml(tool.description ?? '')}</p><small>${tool.risky ? 'risky' : 'standard'}</small></article>`).join('') || '<p class="muted">Tool inventory not loaded.</p>'}</div>`;
}

function renderProviders(): string {
  return `<div class="split"><section><h2>Providers</h2>${renderPre(JSON.stringify(state.providers, null, 2))}</section><section><h2>Models</h2>${renderPre(JSON.stringify(state.models, null, 2))}<h2>Agents</h2>${renderPre(JSON.stringify(state.agents, null, 2))}</section></div>`;
}

function renderSystem(): string {
  return `<div><button id="refresh-system">Refresh system prompt</button>${renderPre(state.systemPrompt || 'No system prompt loaded.')}</div>`;
}

function renderSettings(): string {
  return `
    <form class="settings" id="settings-form">
      ${field('vegvisirBinary', 'Vegvisir binary', state.settings.vegvisirBinary ?? 'vegvisir')}
      ${field('workspace', 'Workspace', state.settings.workspace ?? '')}
      ${field('provider', 'Provider', state.settings.provider ?? '')}
      ${field('model', 'Model', state.settings.model ?? '')}
      ${field('agent', 'Agent', state.settings.agent ?? '')}
      <label class="check"><input type="checkbox" name="dangerousBypass" ${state.settings.dangerousBypass ? 'checked' : ''} /> Dangerous bypass at startup</label>
      <p class="muted">Desktop does not bypass Vegvisir. It spawns <code>vegvisir app-server</code> and uses the bridge so providers, HBSE, CMS, tools, approvals, and policy remain owned by the harness.</p>
      <button class="primary" type="submit">Save settings</button>
    </form>
  `;
}

function field(name: string, label: string, value: string): string {
  return `<label><span>${escapeHtml(label)}</span><input name="${name}" value="${escapeHtml(value)}" /></label>`;
}

function renderPre(value: string): string {
  return `<pre class="panel-pre">${escapeHtml(value)}</pre>`;
}

function bindEvents(): void {
  document.querySelector('#start-stop')?.addEventListener('click', () => state.bridgeRunning ? void stopBridge() : void startBridge());
  document.querySelectorAll<HTMLButtonElement>('[data-panel]').forEach((button) => {
    button.addEventListener('click', () => setPanel(button.dataset.panel ?? 'chat'));
  });
  document.querySelector('#send-turn')?.addEventListener('click', () => void sendTurn());
  document.querySelector('#run-command')?.addEventListener('click', () => void runSlashCommand());
  document.querySelector('#refresh-system')?.addEventListener('click', () => void send('system.prompt', {}, 'system'));
  document.querySelectorAll<HTMLButtonElement>('[data-approval]').forEach((button) => {
    button.addEventListener('click', () => void approve(button.dataset.approval ?? '', button.dataset.method ?? 'approvals.deny'));
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
      dangerousBypass: form.get('dangerousBypass') === 'on',
    };
    saveSettings();
    render();
  });
}

function escapeHtml(value: string): string {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#039;');
}

render();
setInterval(() => void pollBridge(), 350);

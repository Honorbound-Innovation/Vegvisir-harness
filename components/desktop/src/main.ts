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

type ApprovalFeedback = {
  id: string;
  status: 'pending' | 'success' | 'error' | 'denied';
  message: string;
  method: string;
  updatedAt: number;
};

type StartBridgeRequest = {
  workspace?: string;
  provider?: string;
  model?: string;
  agent?: string;
  vegvisirBinary?: string;
  dangerousBypass?: boolean;
  autoStart?: boolean;
  settingsSchemaVersion?: number;
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

type FileExplorerEntry = {
  name: string;
  path: string;
  isDir: boolean;
  isFile: boolean;
  isSymlink: boolean;
  size?: number | null;
  modifiedMs?: number | string | null;
  gitRepo: boolean;
};

type FileExplorerListing = {
  path: string;
  parent?: string | null;
  home?: string | null;
  entries: FileExplorerEntry[];
  truncated?: boolean;
  totalEntries?: number;
  limit?: number;
};

type FileExplorerState = {
  path: string;
  parent: string;
  home: string;
  entries: FileExplorerEntry[];
  loading: boolean;
  error: string;
  selectedPath: string;
  visibleLimit: number;
  truncated: boolean;
  totalEntries: number;
  requestToken: number;
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

type PanelId = 'explorer' | 'chat' | 'sessions' | 'work' | 'approvals' | 'tools' | 'providers' | 'capabilities' | 'commands' | 'runtime' | 'openai' | 'diff' | 'memory' | 'skills' | 'integrations' | 'evidence' | 'system' | 'settings';

type CanvasPanelId = PanelId;

type PanelDefinition = {
  id: PanelId;
  label: string;
  icon: string;
  hint: string;
  defaultWidth: number;
  defaultHeight: number;
  minWidth: number;
  minHeight: number;
};

type CanvasModuleInstance = {
  id: string;
  panelId: CanvasPanelId;
  title: string;
  x: number;
  y: number;
  width: number;
  height: number;
  collapsed: boolean;
  locked: boolean;
  z: number;
};

type CanvasLayoutTab = {
  id: string;
  name: string;
  modules: CanvasModuleInstance[];
};

type CanvasLayoutState = {
  schemaVersion: 1;
  activeTabId: string;
  tabs: CanvasLayoutTab[];
  editMode: boolean;
  gridSize: number;
  snapToGrid: boolean;
};

type CanvasInteraction = {
  mode: 'move' | 'resize';
  id: string;
  startClientX: number;
  startClientY: number;
  startX: number;
  startY: number;
  startWidth: number;
  startHeight: number;
};

const appElement = document.querySelector<HTMLDivElement>('#app');
if (!appElement) throw new Error('missing #app root');
const app = appElement;

const panels: PanelDefinition[] = [
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

const LAYOUT_SCHEMA_VERSION = 1;
const SETTINGS_SCHEMA_VERSION = 2;
const GLOBAL_LAYOUT_STORAGE_KEY = 'vegvisir.desktop.layouts.global';
let loadedLayoutStorageKey = '';
let canvasInteraction: CanvasInteraction | null = null;
let forceChatScrollToBottom = false;

type ChatScrollSnapshot = {
  scrollTop: number;
  bottomOffset: number;
  shouldStickToBottom: boolean;
};

type CanvasScrollSnapshot = {
  scrollLeft: number;
  scrollTop: number;
  tabId: string;
};

const CHAT_SCROLL_STICKY_THRESHOLD_PX = 96;
const APPROVAL_REFRESH_THROTTLE_MS = 750;
const EXPLORER_INITIAL_VISIBLE_ENTRIES = 160;
const EXPLORER_VISIBLE_INCREMENT = 160;
let approvalRefreshInFlight = false;
let lastApprovalRefreshAt = 0;


function panelDefinition(panelId: PanelId): PanelDefinition {
  return panels.find((panel) => panel.id === panelId) ?? panels[0];
}

function defaultLayoutState(): CanvasLayoutState {
  return {
    schemaVersion: LAYOUT_SCHEMA_VERSION,
    activeTabId: 'operations',
    editMode: false,
    gridSize: 24,
    snapToGrid: true,
    tabs: [
      createLayoutTabFromPanels('operations', 'Operations', ['explorer', 'chat', 'work', 'approvals']),
      createLayoutTabFromPanels('provider-control', 'Provider Control', ['providers', 'commands', 'capabilities', 'settings']),
      createLayoutTabFromPanels('review', 'Review', ['diff', 'evidence', 'work', 'sessions']),
      createLayoutTabFromPanels('memory-skills', 'Memory and Skills', ['memory', 'skills', 'system', 'chat']),
      createLayoutTabFromPanels('integrations', 'Integrations', ['integrations', 'tools', 'openai', 'capabilities']),
    ],
  };
}

function createLayoutTabFromPanels(id: string, name: string, panelIds: PanelId[]): CanvasLayoutTab {
  const positions = [
    { x: 32, y: 32 },
    { x: 824, y: 32 },
    { x: 32, y: 688 },
    { x: 584, y: 688 },
  ];
  return {
    id,
    name,
    modules: panelIds.map((panelId, index) => {
      const panel = panelDefinition(panelId);
      const position = positions[index] ?? { x: 32 + index * 48, y: 32 + index * 48 };
      return {
        id: `${id}-${panelId}-${index}`,
        panelId,
        title: panel.label,
        x: position.x,
        y: position.y,
        width: panel.defaultWidth,
        height: panel.defaultHeight,
        collapsed: false,
        locked: false,
        z: index + 1,
      };
    }),
  };
}

function workspaceLayoutStorageKey(): string {
  const workspace = String(state?.session?.workspace ?? state?.settings?.workspace ?? '').trim();
  if (!workspace) return GLOBAL_LAYOUT_STORAGE_KEY;
  const safe = workspace.replace(/[^a-z0-9._-]+/gi, '_').replace(/^_+|_+$/g, '').slice(0, 120);
  return safe ? `vegvisir.desktop.layouts.${safe}` : GLOBAL_LAYOUT_STORAGE_KEY;
}

function loadLayouts(): CanvasLayoutState {
  const key = workspaceLayoutStorageKey();
  loadedLayoutStorageKey = key;
  const raw = localStorage.getItem(key) ?? localStorage.getItem(GLOBAL_LAYOUT_STORAGE_KEY);
  if (!raw) return defaultLayoutState();
  try {
    return normalizeLayoutState(JSON.parse(raw));
  } catch {
    return defaultLayoutState();
  }
}

function normalizeLayoutState(value: Partial<CanvasLayoutState>): CanvasLayoutState {
  const defaults = defaultLayoutState();
  const tabs = Array.isArray(value.tabs)
    ? value.tabs.map(normalizeLayoutTab).filter((tab) => tab.modules.length || tab.name.trim())
    : defaults.tabs;
  const activeTabId = typeof value.activeTabId === 'string' && tabs.some((tab) => tab.id === value.activeTabId)
    ? value.activeTabId
    : (tabs[0]?.id ?? defaults.activeTabId);
  return {
    schemaVersion: LAYOUT_SCHEMA_VERSION,
    activeTabId,
    tabs: tabs.length ? tabs : defaults.tabs,
    editMode: Boolean(value.editMode),
    gridSize: typeof value.gridSize === 'number' && Number.isFinite(value.gridSize) && value.gridSize >= 4 ? value.gridSize : defaults.gridSize,
    snapToGrid: typeof value.snapToGrid === 'boolean' ? value.snapToGrid : defaults.snapToGrid,
  };
}

function normalizeLayoutTab(tab: Partial<CanvasLayoutTab>): CanvasLayoutTab {
  const id = typeof tab.id === 'string' && tab.id.trim() ? tab.id : uniqueId('layout');
  const modules = Array.isArray(tab.modules) ? tab.modules.map(normalizeModuleInstance).filter(Boolean) as CanvasModuleInstance[] : [];
  return {
    id,
    name: typeof tab.name === 'string' && tab.name.trim() ? tab.name : 'Layout',
    modules,
  };
}

function normalizeModuleInstance(module: Partial<CanvasModuleInstance>): CanvasModuleInstance | null {
  if (!isPanelId(module.panelId)) return null;
  const panel = panelDefinition(module.panelId);
  return {
    id: typeof module.id === 'string' && module.id.trim() ? module.id : uniqueId(`module-${module.panelId}`),
    panelId: module.panelId,
    title: typeof module.title === 'string' && module.title.trim() ? module.title : panel.label,
    x: finiteNumber(module.x, 32),
    y: finiteNumber(module.y, 32),
    width: Math.max(panel.minWidth, finiteNumber(module.width, panel.defaultWidth)),
    height: Math.max(panel.minHeight, finiteNumber(module.height, panel.defaultHeight)),
    collapsed: Boolean(module.collapsed),
    locked: Boolean(module.locked),
    z: finiteNumber(module.z, 1),
  };
}

function isPanelId(value: unknown): value is PanelId {
  return typeof value === 'string' && panels.some((panel) => panel.id === value);
}

function finiteNumber(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

function uniqueId(prefix: string): string {
  return `${prefix}-${Date.now()}-${Math.floor(Math.random() * 10000)}`;
}

function saveLayouts(): void {
  const key = workspaceLayoutStorageKey();
  loadedLayoutStorageKey = key;
  localStorage.setItem(key, JSON.stringify(state.layout));
  if (key === GLOBAL_LAYOUT_STORAGE_KEY) localStorage.setItem(GLOBAL_LAYOUT_STORAGE_KEY, JSON.stringify(state.layout));
}

function refreshLayoutStorageScope(): void {
  const key = workspaceLayoutStorageKey();
  if (loadedLayoutStorageKey && key === loadedLayoutStorageKey) return;
  state.layout = loadLayouts();
  canvasInteraction = null;
}

function currentLayout(): CanvasLayoutTab {
  let layout = state.layout.tabs.find((tab) => tab.id === state.layout.activeTabId);
  if (!layout) {
    layout = state.layout.tabs[0] ?? createLayoutTabFromPanels('operations', 'Operations', ['explorer', 'chat', 'work', 'approvals']);
    state.layout.tabs = [layout];
    state.layout.activeTabId = layout.id;
  }
  return layout;
}

function maxModuleZ(layout = currentLayout()): number {
  return layout.modules.reduce((max, module) => Math.max(max, module.z), 0);
}

function snap(value: number): number {
  const gridSize = state.layout.gridSize || 24;
  return state.layout.snapToGrid ? Math.round(value / gridSize) * gridSize : Math.round(value);
}

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
  approvalPaused: false,
  approvalFeedback: null as ApprovalFeedback | null,
  approvalActions: {} as Record<string, ApprovalFeedback>,
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
  fileExplorer: defaultFileExplorerState() as FileExplorerState,
  diff: '',
  memory: '',
  systemPrompt: '',
  activePanel: 'chat' as PanelId,
  layout: defaultLayoutState() as CanvasLayoutState,
  busy: false,
  error: '',
  settings: loadSettings(),
};

state.layout = loadLayouts();

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
  const defaults: StartBridgeRequest = defaultSettings();
  if (!raw) return defaults;
  try {
    const saved = JSON.parse(raw) as StartBridgeRequest;
    const settings = { ...defaults, ...saved };
    // Do not preserve dangerous bypass from older desktop settings. It is a startup-only
    // unsafe mode and must be explicitly re-enabled after this schema migration.
    if (saved.settingsSchemaVersion !== SETTINGS_SCHEMA_VERSION) {
      settings.dangerousBypass = false;
      settings.settingsSchemaVersion = SETTINGS_SCHEMA_VERSION;
      localStorage.setItem('vegvisir.desktop.settings', JSON.stringify(settings));
    }
    return settings;
  } catch {
    return defaults;
  }
}

function defaultSettings(): StartBridgeRequest {
  return {
    vegvisirBinary: 'vegvisir',
    workspace: '',
    provider: '',
    model: '',
    agent: '',
    dangerousBypass: false,
    autoStart: true,
    settingsSchemaVersion: SETTINGS_SCHEMA_VERSION,
  };
}

function defaultFileExplorerState(): FileExplorerState {
  return {
    path: '',
    parent: '',
    home: '',
    entries: [],
    loading: false,
    error: '',
    selectedPath: '',
    visibleLimit: EXPLORER_INITIAL_VISIBLE_ENTRIES,
    truncated: false,
    totalEntries: 0,
    requestToken: 0,
  };
}

function saveSettings(): void {
  state.settings.settingsSchemaVersion = SETTINGS_SCHEMA_VERSION;
  localStorage.setItem('vegvisir.desktop.settings', JSON.stringify(state.settings));
}

function runtimeDangerousBypassEnabled(): boolean {
  return Boolean(
    state.runtimeStatus?.dangerously_bypass_approvals_and_sandbox
    || state.runtimeStatus?.dangerous_bypass
    || state.runtimeStatus?.dangerousBypass
  );
}

function startupDangerousBypassRequested(): boolean {
  return Boolean(state.settings.dangerousBypass);
}

function pendingApprovalCount(): number {
  return Array.isArray(state.approvals) ? state.approvals.length : 0;
}

function isApprovalPaused(): boolean {
  return state.approvalPaused || pendingApprovalCount() > 0;
}

function canSendChatTurn(): boolean {
  return state.bridgeRunning && !state.busy && !isApprovalPaused();
}

function setApprovalsFromBridge(approvals: any[]): void {
  state.approvals = Array.isArray(approvals) ? approvals : [];
  state.approvalPaused = state.approvals.length > 0;
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
    setApprovalsFromBridge([]);
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
  setApprovalsFromBridge([]);
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
    if (lines.length && !canvasInteraction) render();
  } catch (error) {
    state.error = String(error);
    render();
  }
}

function handleEvent(event: BridgeEvent): void {
  if (!event.type && event.payload === undefined && (event as any).error) {
    event = {
      type: 'bridge.response.error',
      id: event.id,
      payload: (event as any).error,
    };
  }
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
      if (!isApprovalPaused()) state.pendingAssistant = '';
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
      if (hasPendingApprovals(event)) {
        state.error = '';
      } else {
        state.error = eventMessage(event, 'turn failed');
      }
      void refreshApprovalRelatedStateThrottled();
      break;
    case 'approval.required':
      state.busy = false;
      state.error = '';
      setApprovalsFromBridge(event.payload?.approvals ?? state.approvals);
      state.session = event.payload?.session ?? state.session;
      if (state.approvals.length) {
        const first = state.approvals[0];
        state.approvalFeedback = {
          id: String(first?.id ?? 'approval'),
          status: 'pending',
          method: 'approval.required',
          message: `${state.approvals.length} approval${state.approvals.length === 1 ? '' : 's'} waiting for your decision.`,
          updatedAt: Date.now(),
        };
      }
      void refreshApprovalRelatedStateThrottled();
      break;
    case 'approvals.list':
      setApprovalsFromBridge(event.payload?.approvals ?? []);
      state.session = event.payload?.session ?? state.session;
      pruneApprovalActionState();
      break;
    case 'approvals.updated':
      handleApprovalsUpdated(event.payload);
      break;
    case 'approval.executed':
      handleApprovalExecuted(event.payload);
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
    case 'bridge.response.error':
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
  if (!content || !canSendChatTurn()) return;
  input!.value = '';
  state.error = '';
  if (content.startsWith('/')) {
    state.messages.push({ role: 'command', content });
    render();
    await send('command.invoke', { command: content }, 'command');
    return;
  }
  state.messages.push({ role: 'user', content });
  state.busy = true;
  state.pendingAssistant = '';
  scrollChatToBottomOnNextRender();
  render();
  try {
    await send('turn.send', { content }, 'turn');
  } catch (error) {
    state.busy = false;
    state.error = String(error);
    render();
  }
}

async function loadSession(id: string): Promise<void> {
  if (!id.trim() || !state.bridgeRunning || state.busy) return;
  await send('session.load', { id: id.trim() }, 'session-load');
}

function setPanel(panel: string): void {
  if (!isPanelId(panel)) return;
  state.activePanel = panel;
  refreshPanelData(panel);
  addOrFocusModule(panel);
}

async function approve(id: string, method: string): Promise<void> {
  if (!id.trim() || !state.bridgeRunning) return;
  state.error = '';
  const approval = state.approvals.find((item) => String(item?.id ?? '') === id);
  const feedback: ApprovalFeedback = {
    id,
    method,
    status: 'pending',
    message: approvalActionStartedMessage(method, approval),
    updatedAt: Date.now(),
  };
  state.approvalActions[id] = feedback;
  state.approvalFeedback = feedback;
  render();
  try {
    await send(method, { id }, 'approval');
    await refreshApprovalRelatedState();
  } catch (error) {
    const failed: ApprovalFeedback = {
      id,
      method,
      status: 'error',
      message: `Approval request failed before reaching the bridge: ${String(error)}`,
      updatedAt: Date.now(),
    };
    state.approvalActions[id] = failed;
    state.approvalFeedback = failed;
    state.error = failed.message;
    render();
  }
}

function approvalActionStartedMessage(method: string, approval: any): string {
  const toolName = approval?.tool_name ?? approval?.toolName ?? 'approval request';
  if (method === 'approvals.deny') return `Denying ${toolName}…`;
  if (method === 'approvals.approveSessionAndExecute') return `Approving ${toolName} for this session and executing it…`;
  if (method === 'approvals.approveOnceAndExecute') return `Approving ${toolName} once and executing it…`;
  return `Updating ${toolName}…`;
}

async function refreshApprovalRelatedState(): Promise<void> {
  await Promise.allSettled([
    send('approvals.list', {}, 'approvals'),
    send('session.status', {}, 'status'),
  ]);
}

async function refreshApprovalRelatedStateThrottled(): Promise<void> {
  const now = Date.now();
  if (approvalRefreshInFlight || now - lastApprovalRefreshAt < APPROVAL_REFRESH_THROTTLE_MS) return;
  approvalRefreshInFlight = true;
  lastApprovalRefreshAt = now;
  try {
    await refreshApprovalRelatedState();
  } finally {
    approvalRefreshInFlight = false;
  }
}

function pruneApprovalActionState(): void {
  const pendingIds = new Set(state.approvals.map((approval) => String(approval?.id ?? '')).filter(Boolean));
  for (const [id, feedback] of Object.entries(state.approvalActions)) {
    if (!pendingIds.has(id) && feedback.status === 'pending') continue;
    if (!pendingIds.has(id) && Date.now() - feedback.updatedAt > 30_000) delete state.approvalActions[id];
  }
}

function handleApprovalExecuted(payload: any): void {
  const approval = payload?.approval;
  const active = mostRecentApprovalAction();
  const id = String(approval?.id ?? payload?.id ?? active?.id ?? 'approval');
  const error = approvalExecutionError(payload?.observation);
  const feedback: ApprovalFeedback = {
    id,
    method: 'approval.executed',
    status: payload?.ok && !error ? 'success' : 'error',
    message: approvalExecutionMessage(payload),
    updatedAt: Date.now(),
  };
  setApprovalsFromBridge(payload?.approvals ?? []);
  state.session = payload?.session ?? state.session;
  state.error = error ?? '';
  state.approvalActions[id] = feedback;
  state.approvalFeedback = feedback;
  state.messages.push({
    role: feedback.status === 'error' ? 'system' : 'assistant',
    content: feedback.message,
  });
  scrollChatToBottomOnNextRender();
  void refreshApprovalRelatedState();
}

function handleApprovalsUpdated(payload: any): void {
  setApprovalsFromBridge(payload?.approvals ?? []);
  state.session = payload?.session ?? state.session;
  const active = mostRecentApprovalAction();
  if (active && active.method === 'approvals.deny') {
    const feedback: ApprovalFeedback = {
      ...active,
      status: payload?.ok === false ? 'error' : 'denied',
      message: payload?.ok === false ? 'Approval denial was not applied.' : 'Approval denied. Pending approval list refreshed.',
      updatedAt: Date.now(),
    };
    state.approvalActions[feedback.id] = feedback;
    state.approvalFeedback = feedback;
  }
  pruneApprovalActionState();
}

function mostRecentApprovalAction(): ApprovalFeedback | null {
  return Object.values(state.approvalActions)
    .sort((left, right) => right.updatedAt - left.updatedAt)[0] ?? null;
}

function hasPendingApprovals(event: BridgeEvent): boolean {
  const approvals = event.payload?.approvals ?? event.payload?.session?.pending_approvals;
  if (Array.isArray(approvals)) return approvals.length > 0;
  if (typeof approvals === 'number') return approvals > 0;
  const message = eventMessage(event, '').toLowerCase();
  return message.includes('approvalrequired') || message.includes('approval required') || message.includes('approval_id=');
}

function approvalExecutionError(observation: unknown): string | null {
  if (!observation || typeof observation !== 'object') return null;
  const value = observation as { ok?: unknown; error?: unknown; content?: unknown };
  if (value.ok !== false && !value.error) return null;
  const error = typeof value.error === 'string' ? value.error : 'Approved tool execution failed';
  const content = typeof value.content === 'string' ? value.content : '';
  return [error, content].filter(Boolean).join(': ');
}

function approvalExecutionMessage(payload: any): string {
  const approval = payload?.approval;
  const observation = payload?.observation;
  const toolName = approval?.tool_name ?? approval?.toolName ?? 'approved tool';
  if (!payload?.ok) return `Approval was not applied. The request may already be gone or was resolved elsewhere.`;
  if (payload?.continued) return `Approved ${toolName}; resuming the model turn so the approved tool result goes back into chat.`;
  if (!observation || typeof observation !== 'object') return `Approved and executed ${toolName}.`;
  const status = observation.ok === false ? 'failed' : 'completed';
  const content = typeof observation.content === 'string' && observation.content.trim()
    ? `

${observation.content.trim()}`
    : '';
  return `Approved ${toolName}; execution ${status}.${content}`;
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

function nativeMethodNames(): string[] {
  const methods = state.capabilities?.native_methods;
  return Array.isArray(methods) ? methods.map(String) : [];
}

function commandBackedMethodNames(): string[] {
  const methods = state.capabilities?.command_backed_methods;
  if (!Array.isArray(methods)) return [];
  return methods.map((method: any) => typeof method === 'string' ? method : String(method?.method ?? '')).filter(Boolean);
}

function hasNativeMethod(method: string): boolean {
  return nativeMethodNames().includes(method);
}

function hasCommandBackedMethod(method: string): boolean {
  return commandBackedMethodNames().includes(method);
}

function hasBridgeMethod(method: string): boolean {
  if (!state.capabilities) return true;
  return hasNativeMethod(method) || hasCommandBackedMethod(method) || method === 'command.invoke';
}

function bridgeMethodDisabledAttr(method: string): string {
  return hasBridgeMethod(method) ? '' : 'disabled title="Bridge capability not reported by app-server"';
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

function captureChatScrollSnapshot(): ChatScrollSnapshot | null {
  const surface = document.querySelector<HTMLDivElement>('#chat-scroll-surface');
  if (!surface) return null;
  const bottomOffset = surface.scrollHeight - surface.scrollTop - surface.clientHeight;
  return {
    scrollTop: surface.scrollTop,
    bottomOffset,
    shouldStickToBottom: forceChatScrollToBottom || bottomOffset <= CHAT_SCROLL_STICKY_THRESHOLD_PX,
  };
}

function restoreChatScroll(snapshot: ChatScrollSnapshot | null): void {
  requestAnimationFrame(() => {
    const surface = document.querySelector<HTMLDivElement>('#chat-scroll-surface');
    if (!surface) return;
    if (!snapshot || snapshot.shouldStickToBottom) {
      surface.scrollTop = surface.scrollHeight;
    } else {
      surface.scrollTop = Math.max(0, Math.min(snapshot.scrollTop, surface.scrollHeight - surface.clientHeight));
    }
    forceChatScrollToBottom = false;
  });
}

function captureCanvasScrollSnapshot(): CanvasScrollSnapshot | null {
  const surface = document.querySelector<HTMLDivElement>('#canvas-surface');
  if (!surface) return null;
  return {
    scrollLeft: surface.scrollLeft,
    scrollTop: surface.scrollTop,
    tabId: surface.dataset.canvasTab ?? '',
  };
}

function restoreCanvasScroll(snapshot: CanvasScrollSnapshot | null): void {
  if (!snapshot) return;
  const apply = () => {
    const surface = document.querySelector<HTMLDivElement>('#canvas-surface');
    if (!surface || surface.dataset.canvasTab !== snapshot.tabId) return;
    surface.scrollLeft = Math.max(0, Math.min(snapshot.scrollLeft, surface.scrollWidth - surface.clientWidth));
    surface.scrollTop = Math.max(0, Math.min(snapshot.scrollTop, surface.scrollHeight - surface.clientHeight));
  };
  apply();
  requestAnimationFrame(apply);
}

function scrollChatToBottomOnNextRender(): void {
  forceChatScrollToBottom = true;
}

function render(): void {
  const chatScrollSnapshot = captureChatScrollSnapshot();
  const canvasScrollSnapshot = captureCanvasScrollSnapshot();
  refreshLayoutStorageScope();
  app.innerHTML = `
    <div class="grid h-screen grid-cols-[18rem_minmax(0,1fr)] overflow-hidden bg-vv-bg bg-vv-radial text-vv-text selection:bg-vv-cyan/25 max-[980px]:grid-cols-1">
      ${renderLeftRail()}
      <main class="grid min-h-0 min-w-0 grid-rows-[3.75rem_minmax(0,1fr)_1.75rem] overflow-hidden border-l border-vv-line bg-vv-bg2/74 max-[980px]:border-l-0">
        ${renderTopBar()}
        <section class="flex min-h-0 flex-col overflow-hidden bg-vv-grid [background-size:42px_42px]">
          ${state.error ? renderError() : ''}
          ${renderApprovalToast()}
          ${renderPanel()}
        </section>
        ${renderFooterRail()}
      </main>
    </div>
  `;
  bindEvents();
  restoreChatScroll(chatScrollSnapshot);
  restoreCanvasScroll(canvasScrollSnapshot);
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
          ${renderModelWorkingPill()}
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
      <span class="inline-flex items-center gap-1.5 ${isApprovalPaused() ? 'text-vv-amber' : state.busy ? 'text-vv-cyan' : ''}">${state.busy || isApprovalPaused() ? '<span class="vv-mini-spinner" aria-hidden="true"></span>' : ''}${state.busy || isApprovalPaused() ? modelWorkingLabel().toLowerCase() : 'ready'} · main</span>
    </footer>
  `;
}

function renderError(): string {
  return `<div class="mx-auto mt-3 max-w-5xl rounded-xl border border-vv-red/45 bg-vv-red/10 p-3 text-red-100 shadow-danger"><div class="flex flex-wrap items-center justify-between gap-2"><strong>Bridge problem</strong><button class="vv-action vv-action-danger" id="restart-bridge-from-error">Restart bridge</button></div><pre class="vv-code mt-2 whitespace-pre-wrap">${escapeHtml(state.error)}</pre></div>`;
}

function modelWorkingLabel(): string {
  if (isApprovalPaused()) return 'Paused for approval';
  return state.pendingAssistant.trim() ? 'Streaming response' : 'Model working';
}

function renderModelWorkingPill(label = modelWorkingLabel()): string {
  if (!state.busy && !isApprovalPaused()) return '';
  return `<span class="vv-pill vv-working-pill ${isApprovalPaused() ? 'text-vv-amber' : 'text-vv-cyan'}" role="status" aria-live="polite"><span class="vv-mini-spinner" aria-hidden="true"></span>${escapeHtml(label)}</span>`;
}

function renderModelWorkingIndicator(): string {
  if (!state.busy && !isApprovalPaused()) return '';
  const label = modelWorkingLabel();
  const detail = isApprovalPaused()
    ? 'The model turn is stopped at a human approval gate. Decide the pending approval before sending another chat turn.'
    : state.pendingAssistant.trim()
      ? 'Tokens are streaming into the assistant response.'
      : 'Waiting for the provider, tools, or approval-aware continuation to produce the next chat update.';
  return `
    <article class="vv-soft-panel vv-working-card max-w-4xl border-vv-cyan/35 bg-vv-cyan/5" role="status" aria-live="polite" aria-label="${escapeHtml(label)}">
      <div class="flex items-center gap-3 px-4 py-3">
        <span class="vv-chat-spinner" aria-hidden="true"></span>
        <div class="min-w-0">
          <div class="flex items-center gap-2 text-sm font-black text-vv-cyan">${escapeHtml(label)}<span class="vv-working-dots" aria-hidden="true"><span></span><span></span><span></span></span></div>
          <div class="mt-1 text-xs leading-5 text-vv-muted">${escapeHtml(detail)}</div>
        </div>
      </div>
    </article>`;
}

function renderApprovalToast(): string {
  const feedback = state.approvalFeedback;
  if (!feedback) return '';
  const tone = feedback.status === 'success'
    ? 'border-vv-green/45 bg-vv-green/10 text-green-100'
    : feedback.status === 'error'
      ? 'border-vv-red/45 bg-vv-red/10 text-red-100 shadow-danger'
      : feedback.status === 'denied'
        ? 'border-vv-amber/45 bg-vv-amber/10 text-amber-100'
        : 'border-vv-cyan/45 bg-vv-cyan/10 text-cyan-100 shadow-glow';
  const dot = feedback.status === 'success'
    ? 'bg-vv-green'
    : feedback.status === 'error'
      ? 'bg-vv-red'
      : feedback.status === 'denied'
        ? 'bg-vv-amber'
        : 'bg-vv-cyan animate-pulse';
  return `
    <div class="mx-auto mt-3 w-[calc(100%-2rem)] max-w-5xl rounded-xl border ${tone} p-3 text-sm">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <div class="flex min-w-0 items-center gap-2">
          <span class="h-2.5 w-2.5 shrink-0 rounded-full ${dot}"></span>
          <span class="font-semibold">${escapeHtml(feedback.message)}</span>
        </div>
        <button class="vv-action px-2 py-1 text-[0.68rem]" id="clear-approval-feedback">Dismiss</button>
      </div>
    </div>`;
}

function renderPanel(): string {
  return renderCanvas();
}

function renderNonChatPanel(): string {
  return renderModuleContent(state.activePanel);
}

function renderCanvas(): string {
  const layout = currentLayout();
  const bounds = canvasBounds(layout.modules);
  return `
    <div class="grid min-h-0 flex-1 grid-rows-[auto_minmax(0,1fr)] overflow-hidden">
      ${renderLayoutTabs()}
      <div class="grid min-h-0 grid-cols-1 overflow-hidden">
        <div id="canvas-surface" data-canvas-tab="${escapeHtml(layout.id)}" class="vv-scrollbar relative min-h-0 overflow-auto bg-vv-grid [background-size:42px_42px]">
          <div id="canvas-content" class="relative" style="width:${bounds.width}px;height:${bounds.height}px;min-width:100%;min-height:100%;">
            ${layout.modules.map(renderModuleFrame).join('')}
            ${layout.modules.length ? '' : renderBlankCanvasEmpty()}
          </div>
        </div>
      </div>
    </div>`;
}

function renderLayoutTabs(): string {
  const layout = currentLayout();
  const pendingApprovals = state.approvals.length > 0;
  const dangerous = Boolean(startupDangerousBypassRequested() || runtimeDangerousBypassEnabled());
  return `
    <div class="border-b border-vv-line bg-vv-bg2/88 px-4 py-2 backdrop-blur-xl">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div class="vv-scrollbar flex min-w-0 flex-1 items-center gap-2 overflow-x-auto pb-1">
          ${state.layout.tabs.map((tab) => `<button class="vv-action shrink-0 ${tab.id === state.layout.activeTabId ? 'vv-action-primary' : ''}" data-layout-tab="${escapeHtml(tab.id)}">${escapeHtml(tab.name)}</button>`).join('')}
          <button class="vv-action shrink-0" id="layout-add-tab">+</button>
        </div>
        <div class="flex flex-wrap items-center gap-2">
          <span class="vv-pill ${state.bridgeRunning ? 'text-vv-green' : 'text-vv-red'}"><span class="h-2 w-2 rounded-full ${state.bridgeRunning ? 'bg-vv-green' : 'bg-vv-red'}"></span>${state.bridgeRunning ? 'Bridge online' : 'Bridge offline'}</span>
          ${pendingApprovals ? `<button class="vv-action vv-action-danger" data-module-add="approvals">${state.approvals.length} approvals</button>` : ''}
          ${dangerous ? '<span class="vv-pill text-vv-red">dangerous bypass startup mode</span>' : ''}
          <button class="vv-action" id="layout-save">Save</button>
          <button class="vv-action" id="layout-duplicate-tab">Duplicate</button>
          <button class="vv-action" id="layout-rename-tab">Rename</button>
          <button class="vv-action" id="layout-delete-tab">Delete</button>
          <button class="vv-action" id="layout-reset-tab">Reset tab</button>
        </div>
      </div>
      <div class="mt-2 flex flex-wrap items-center justify-between gap-2 text-xs text-vv-muted">
        <span>${escapeHtml(layout.name)} · ${layout.modules.length} modules · drag, resize, add, and remove enabled</span>
        <label class="flex items-center gap-2"><input id="layout-snap" type="checkbox" ${state.layout.snapToGrid ? 'checked' : ''} /> snap ${state.layout.gridSize}px grid</label>
      </div>
    </div>`;
}

function canvasBounds(modules: CanvasModuleInstance[]): { width: number; height: number } {
  if (!modules.length) return { width: 1, height: 1 };
  const maxX = modules.reduce((max, module) => Math.max(max, module.x + module.width), 0);
  const maxY = modules.reduce((max, module) => Math.max(max, module.y + module.height), 0);
  return { width: Math.max(1, Math.ceil(maxX)), height: Math.max(1, Math.ceil(maxY)) };
}

function renderBlankCanvasEmpty(): string {
  return `
    <div class="absolute inset-0 grid min-h-[34rem] place-items-center p-8 text-center">
      <section class="vv-panel max-w-xl p-6 shadow-glow">
        <div class="mx-auto mb-4 grid h-14 w-14 place-items-center rounded-3xl border border-vv-line bg-white/[0.045] text-2xl text-vv-cyan">▦</div>
        <h2 class="text-xl font-black">Blank canvas</h2>
        <p class="mt-2 text-sm leading-6 text-vv-muted">Add verified Vegvisir modules from the palette. Layout state is local only; bridge/runtime authority remains in Vegvisir app-server.</p>
        <button class="vv-action vv-action-primary mt-4" data-module-add="chat">Add Chat</button>
      </section>
    </div>`;
}

function renderModuleFrame(instance: CanvasModuleInstance): string {
  const panel = panelDefinition(instance.panelId);
  const mode = moduleRenderMode(instance);
  const selected = instance.z === maxModuleZ();
  const draggable = !instance.locked;
  const isApproval = instance.panelId === 'approvals' && state.approvals.length > 0;
  const isRuntimeDanger = instance.panelId === 'runtime' && Boolean(startupDangerousBypassRequested() || runtimeDangerousBypassEnabled());
  return `
    <article data-module-frame="${escapeHtml(instance.id)}" class="vv-canvas-module vv-panel absolute overflow-hidden ${selected ? 'vv-canvas-module-selected' : ''} ${instance.locked ? 'vv-canvas-module-locked' : ''} ${mode === 'compact' ? 'vv-canvas-module-compact' : ''} ${mode === 'collapsed' ? 'vv-canvas-module-collapsed' : ''} ${isApproval || isRuntimeDanger ? 'border-vv-red/60 shadow-danger' : ''}" style="left:${instance.x}px;top:${instance.y}px;width:${instance.width}px;height:${instance.height}px;z-index:${instance.z};">
      <header class="vv-canvas-module-header ${draggable ? 'vv-canvas-module-draggable' : ''} flex select-none items-center justify-between gap-2 border-b border-vv-line bg-black/32 px-3 py-2" data-module-drag="${escapeHtml(instance.id)}">
        <div class="flex min-w-0 items-center gap-2">
          <span class="grid h-7 w-7 shrink-0 place-items-center rounded-lg border border-vv-line bg-white/[0.045] text-vv-cyan">${panel.icon}</span>
          <div class="min-w-0">
            <h3 class="truncate text-sm font-black">${escapeHtml(instance.title)}</h3>
            <p class="truncate text-[0.68rem] text-vv-muted">${escapeHtml(panel.hint)}${instance.locked ? ' · locked' : ''}</p>
          </div>
        </div>
        <div class="flex shrink-0 items-center gap-1">
          <button class="vv-action px-2 py-1 text-[0.65rem]" data-module-focus="${escapeHtml(instance.id)}">Focus</button>
          <button class="vv-action px-2 py-1 text-[0.65rem]" data-module-collapse="${escapeHtml(instance.id)}">${mode === 'collapsed' ? '□' : '—'}</button>
          <button class="vv-action px-2 py-1 text-[0.65rem]" data-module-lock="${escapeHtml(instance.id)}">${instance.locked ? '🔒' : '◇'}</button>
          <button class="vv-action vv-action-danger px-2 py-1 text-[0.65rem]" data-module-remove="${escapeHtml(instance.id)}">×</button>
        </div>
      </header>
      ${mode === 'collapsed' ? '' : `<div class="vv-scrollbar h-[calc(100%-2.95rem)] overflow-auto p-3 ${mode === 'compact' ? 'bg-black/10' : ''}" data-panel-root="${escapeHtml(instance.panelId)}">${mode === 'compact' ? renderModuleCompact(instance.panelId) : renderModuleContent(instance.panelId)}</div>`}
      ${draggable ? `<div class="vv-canvas-resize-handle" data-module-resize="${escapeHtml(instance.id)}" title="Resize module"></div>` : ''}
    </article>`;
}

function moduleRenderMode(instance: CanvasModuleInstance): 'full' | 'compact' | 'collapsed' {
  if (instance.collapsed || instance.height < 96) return 'collapsed';
  if (instance.width < 380 || instance.height < 260) return 'compact';
  return 'full';
}

function renderModuleContent(panelId: PanelId): string {
  switch (panelId) {
    case 'explorer': return renderExplorer();
    case 'chat': return renderChat();
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
  }
}

function renderModuleCompact(panelId: PanelId): string {
  const text = moduleSummary(panelId);
  return `<div class="space-y-3 text-sm leading-6 text-vv-muted"><p>${escapeHtml(text)}</p><button class="vv-action" data-module-open-full="${panelId}">Open / focus full module</button></div>`;
}

function moduleSummary(panelId: PanelId): string {
  switch (panelId) {
    case 'explorer': return `${state.fileExplorer.path || state.settings.workspace || 'home'} · ${state.fileExplorer.entries.length} entries`;
    case 'chat': return `${state.busy ? 'Busy' : 'Ready'} · ${state.messages.length} messages${state.pendingAssistant ? ' · streaming' : ''}`;
    case 'sessions': return `${state.sessions.length} sessions loaded`;
    case 'work': return `${state.events.length} bridge events`;
    case 'approvals': return `${state.approvals.length} pending approvals`;
    case 'tools': return `${state.tools.length} tools loaded`;
    case 'providers': return `Provider ${state.session?.provider ?? state.settings.provider ?? 'default'} · Model ${state.session?.model ?? state.settings.model ?? 'default'} · ${state.agents.length} agents`;
    case 'capabilities': return `${state.capabilities?.native_methods?.length ?? 0} native methods · ${state.capabilities?.command_backed_methods?.length ?? 0} command-backed methods`;
    case 'commands': return `${state.commands.length} slash commands loaded`;
    case 'runtime': return `${state.bridgeRunning ? 'Bridge online' : 'Bridge offline'} · ${state.busy ? 'working' : 'ready'} · ${startupDangerousBypassRequested() || runtimeDangerousBypassEnabled() ? 'dangerous startup bypass set' : 'policy gated'}`;
    case 'openai': return state.openaiCompat ? 'OpenAI-compatible metadata loaded; credentials remain behind Vegvisir/HBSE.' : 'OpenAI-compatible metadata not loaded.';
    case 'diff': return state.diff ? 'Diff loaded' : 'No diff loaded';
    case 'memory': return state.memory || methodOutputText('memory.status') ? 'Memory status output loaded' : 'Memory output not loaded';
    case 'skills': return methodOutputText('skills.status') ? 'Skills output loaded' : 'Skills output not loaded';
    case 'integrations': return [methodOutputText('mcp.status'), methodOutputText('hbse.status'), methodOutputText('subagents.list')].some(Boolean) ? 'Integration output loaded' : 'Integration output not loaded';
    case 'evidence': return [methodOutputText('runs.list'), methodOutputText('trace.list'), methodOutputText('verify.run')].some(Boolean) ? 'Evidence output loaded' : 'Evidence output not loaded';
    case 'system': return state.systemPrompt ? 'System prompt loaded' : 'System prompt not loaded';
    case 'settings': return `Workspace ${state.settings.workspace || 'default'} · binary ${state.settings.vegvisirBinary || 'vegvisir'}`;
  }
}

function renderChat(): string {
  const messages = [...state.messages];
  if (state.pendingAssistant) messages.push({ role: 'assistant', content: state.pendingAssistant });
  const transcript = messages.length ? messages.map(renderMessage).join('') : renderEmptyTranscript();
  return `
    <div class="grid h-full max-h-full min-h-0 grid-rows-[minmax(0,1fr)_auto] overflow-hidden">
      <div id="chat-scroll-surface" class="vv-scrollbar min-h-0 overflow-y-auto overflow-x-hidden px-5 py-5">
        <div class="mx-auto max-w-4xl space-y-5 pb-4">
          ${transcript}
          ${renderModelWorkingIndicator()}
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
  const approvalPaused = isApprovalPaused();
  const canSend = canSendChatTurn();
  const placeholder = approvalPaused
    ? 'Approval required before the next chat turn can be sent.'
    : 'Ask Vegvisir anything, or type /sessions, /load <id>, /tools, /diff...';
  const readyPill = approvalPaused
    ? `<span class="vv-pill text-vv-amber">Paused · ${pendingApprovalCount()} approval${pendingApprovalCount() === 1 ? '' : 's'}</span>`
    : '<span class="vv-pill">Ready</span>';
  return `
    <div class="mx-auto w-full max-w-4xl">
      <div class="rounded-[1.15rem] border border-vv-line2 bg-vv-panel/90 p-2.5 shadow-[0_18px_56px_rgba(0,0,0,0.30)]">
        <textarea id="turn-input" class="vv-focus vv-scrollbar h-16 max-h-16 min-h-16 w-full resize-none rounded-xl border border-transparent bg-transparent px-2.5 py-1.5 text-[0.92rem] leading-6 text-vv-text placeholder:text-vv-dim" placeholder="${escapeHtml(placeholder)}" ${state.bridgeRunning && !approvalPaused ? '' : 'disabled'}></textarea>
        <div class="mt-1.5 flex items-center justify-between gap-2 border-t border-vv-line pt-2">
          <div class="flex min-w-0 items-center gap-1.5 overflow-hidden text-xs text-vv-muted">
            <span class="vv-pill">${escapeHtml(state.settings.model || 'model default')}</span>
            ${state.busy || approvalPaused ? renderModelWorkingPill() : readyPill}
            <span class="vv-pill">Chat + slash commands</span>
            <span class="vv-pill">${startupDangerousBypassRequested() || runtimeDangerousBypassEnabled() ? 'Bypass startup' : 'Policy gated'}</span>
          </div>
          <button id="send-turn" class="vv-focus grid h-9 w-9 shrink-0 place-items-center rounded-full ${state.busy || approvalPaused ? 'bg-vv-red' : 'bg-vv-pink'} text-lg font-black text-white shadow-[0_0_28px_rgba(255,46,126,0.28)]" ${canSend ? '' : 'disabled'}>${state.busy || approvalPaused ? '■' : '➤'}</button>
        </div>
      </div>
      <div class="mt-2 flex flex-wrap items-center justify-between gap-2 text-xs text-vv-muted">
        <span>${approvalPaused ? 'Approve or deny the pending risk gate before sending another turn. This prevents approval pile-ups.' : 'Slash commands run from this same input. Press <kbd class="rounded border border-vv-line px-1 text-vv-dim">Enter</kbd> to send, <kbd class="rounded border border-vv-line px-1 text-vv-dim">Shift+Enter</kbd> for newline.'}</span>
        <button class="vv-action" data-panel="${approvalPaused ? 'approvals' : 'sessions'}" ${state.bridgeRunning ? '' : 'disabled'}>${approvalPaused ? 'Open approvals' : 'Load session'}</button>
      </div>
    </div>
  `;
}

type MarkdownSegment =
  | { kind: 'text'; text: string }
  | { kind: 'code'; language: string; code: string };

const CODE_FENCE_RE = /```([^\n`]*)\n?([\s\S]*?)(?:```|$)/g;
const INLINE_CODE_RE = /`([^`]+)`/g;
type SyntaxKind =
  | 'comment'
  | 'keyword'
  | 'number'
  | 'string'
  | 'function'
  | 'type'
  | 'property'
  | 'builtin'
  | 'operator'
  | 'punctuation'
  | 'diff-add'
  | 'diff-delete'
  | 'diff-meta'
  | 'tag'
  | 'attr';

const COMMON_CODE_KEYWORDS = new Set([
  'abstract', 'as', 'async', 'await', 'bool', 'boolean', 'break', 'case', 'catch', 'char', 'class', 'const', 'continue', 'def', 'default',
  'do', 'else', 'enum', 'export', 'extends', 'false', 'final', 'finally', 'float', 'fn', 'for', 'from', 'function', 'if', 'impl',
  'import', 'in', 'int', 'interface', 'let', 'match', 'mod', 'mut', 'namespace', 'new', 'none', 'null', 'package', 'pass', 'private',
  'protected', 'public', 'return', 'self', 'static', 'struct', 'super', 'switch', 'this', 'throw', 'trait', 'true', 'try', 'type',
  'undefined', 'use', 'using', 'var', 'void', 'while', 'yield', 'where', 'with', 'readonly', 'record', 'sealed', 'virtual', 'override',
]);

const LANGUAGE_CODE_KEYWORDS: Record<string, string[]> = {
  csharp: ['base', 'decimal', 'delegate', 'dynamic', 'event', 'get', 'global', 'internal', 'is', 'lock', 'object', 'out', 'params', 'partial', 'ref', 'required', 'set', 'sizeof', 'stackalloc', 'string', 'typeof', 'uint', 'ulong', 'unchecked', 'unsafe'],
  css: ['and', 'from', 'important', 'media', 'not', 'or', 'supports', 'to'],
  html: ['doctype'],
  java: ['assert', 'byte', 'double', 'implements', 'instanceof', 'long', 'native', 'short', 'strictfp', 'synchronized', 'throws', 'transient', 'volatile'],
  javascript: ['debugger', 'delete', 'instanceof', 'of'],
  json: [],
  markdown: [],
  python: ['and', 'assert', 'del', 'elif', 'except', 'global', 'is', 'lambda', 'nonlocal', 'not', 'or', 'raise'],
  rust: ['crate', 'dyn', 'extern', 'loop', 'move', 'pub', 'ref', 'unsafe'],
  shell: ['alias', 'case', 'cd', 'done', 'elif', 'esac', 'eval', 'exec', 'exit', 'fi', 'local', 'read', 'then'],
  typescript: ['declare', 'keyof', 'satisfies', 'symbol', 'unique'],
  yaml: ['true', 'false', 'null'],
};

const CODE_BUILTINS = new Set([
  'Array', 'Boolean', 'Console', 'Date', 'Dict', 'Error', 'Exception', 'JSON', 'List', 'Map', 'Math', 'Number', 'Object', 'Promise', 'Record',
  'Regex', 'RegExp', 'Set', 'String', 'Vec', 'console', 'dict', 'enumerate', 'len', 'list', 'map', 'print', 'range', 'str', 'sum', 'println',
]);

function renderMessage(message: Message): string {
  const role = message.role ?? 'message';
  const isUser = role === 'user';
  const isCommand = role === 'command';
  const isTool = role.includes('tool') || role.includes('event');
  const cardClass = isUser ? 'ml-auto max-w-3xl bg-white/[0.07]' : isCommand ? 'max-w-4xl border-vv-cyan/35 bg-vv-cyan/5' : isTool ? 'max-w-4xl border-vv-line bg-black/18 opacity-75' : 'max-w-4xl bg-white/[0.035]';
  return `
    <article class="vv-soft-panel ${cardClass}">
      <header class="flex items-center gap-3 border-b border-vv-line px-4 py-2 text-[0.68rem] uppercase tracking-[0.22em] text-vv-muted"><span class="h-2 w-2 rounded-full ${isUser ? 'bg-vv-green' : isCommand ? 'bg-vv-cyan' : 'bg-vv-cyan'}"></span>${escapeHtml(role)}</header>
      <div class="px-4 py-3">${renderMessageMarkdown(message.content ?? message.text ?? '')}</div>
    </article>
  `;
}

function renderMessageMarkdown(markdown: string): string {
  const segments = parseMarkdownSegments(markdown);
  if (!segments.length) return '<div class="vv-chat-markdown text-vv-muted">(empty)</div>';
  return segments.map((segment) => segment.kind === 'code'
    ? renderCodeFence(segment.code, segment.language)
    : renderMarkdownText(segment.text)
  ).join('');
}

function parseMarkdownSegments(markdown: string): MarkdownSegment[] {
  const segments: MarkdownSegment[] = [];
  let cursor = 0;
  CODE_FENCE_RE.lastIndex = 0;
  for (const match of markdown.matchAll(CODE_FENCE_RE)) {
    const start = match.index ?? 0;
    if (start > cursor) segments.push({ kind: 'text', text: markdown.slice(cursor, start) });
    segments.push({ kind: 'code', language: normalizeCodeLanguage(match[1] ?? ''), code: stripTrailingFenceNewline(match[2] ?? '') });
    cursor = start + match[0].length;
  }
  if (cursor < markdown.length) segments.push({ kind: 'text', text: markdown.slice(cursor) });
  return segments.filter((segment) => segment.kind === 'code' || segment.text.length > 0);
}

function stripTrailingFenceNewline(code: string): string {
  return code.replace(/^\n/, '').replace(/\n$/, '');
}

function normalizeCodeLanguage(language: string): string {
  const normalized = language.trim().split(/\s+/)[0]?.toLowerCase() ?? '';
  const aliases: Record<string, string> = {
    csharp: 'csharp', cs: 'csharp', js: 'javascript', jsx: 'javascript', py: 'python', rs: 'rust', sh: 'shell', bash: 'shell',
    ts: 'typescript', tsx: 'typescript', yml: 'yaml', md: 'markdown', diff: 'diff', patch: 'diff', plaintext: 'text', plain: 'text',
  };
  return aliases[normalized] ?? normalized;
}

function renderMarkdownText(text: string): string {
  if (!text.trim()) return '';
  const blocks = text.replace(/^\n+|\n+$/g, '').split(/\n{2,}/);
  return blocks.map((block) => `<div class="vv-chat-markdown">${renderInlineMarkdown(block).replaceAll('\n', '<br />')}</div>`).join('');
}

function renderInlineMarkdown(text: string): string {
  return escapeHtml(text).replace(INLINE_CODE_RE, (_match, code: string) => `<code class="vv-chat-inline-code">${code}</code>`);
}

function renderCodeFence(code: string, language: string): string {
  const label = language || 'code';
  return `
    <section class="vv-chat-code-block">
      <div class="vv-chat-code-header"><span>${escapeHtml(label)}</span></div>
      <pre class="vv-code vv-scrollbar"><code class="language-${escapeHtml(label)}">${highlightCode(code, language)}</code></pre>
    </section>`;
}

function highlightCode(code: string, language: string): string {
  const mode = normalizeCodeLanguage(language);
  if (mode === 'diff') return highlightDiffCode(code);
  if (mode === 'html' || mode === 'xml') return highlightMarkupCode(code);

  const keywords = keywordsForLanguage(mode);
  let output = '';
  for (let index = 0; index < code.length;) {
    const rest = code.slice(index);

    const blockComment = rest.match(/^\/\*[\s\S]*?\*\//);
    if (blockComment) {
      output += syntaxToken(blockComment[0], 'comment');
      index += blockComment[0].length;
      continue;
    }

    const lineComment = rest.match(lineCommentPattern(mode));
    if (lineComment) {
      output += syntaxToken(lineComment[0], 'comment');
      index += lineComment[0].length;
      continue;
    }

    const stringLiteral = rest.match(stringLiteralPattern(mode));
    if (stringLiteral) {
      const token = stringLiteral[0];
      const after = code.slice(index + token.length).match(/^\s*:/);
      output += (after && ['json', 'yaml'].includes(mode)) ? syntaxToken(token, 'property') : syntaxToken(token, 'string');
      index += token.length;
      continue;
    }

    const numberLiteral = rest.match(/^\b(?:0b[01_]+|0o[0-7_]+|0x[0-9a-fA-F_]+|\d[\d_]*(?:\.\d[\d_]*)?)(?:[eE][+-]?\d[\d_]*)?\b/);
    if (numberLiteral) {
      output += syntaxToken(numberLiteral[0], 'number');
      index += numberLiteral[0].length;
      continue;
    }

    const decorator = rest.match(/^@[A-Za-z_][A-Za-z0-9_.-]*/);
    if (decorator) {
      output += syntaxToken(decorator[0], 'attr');
      index += decorator[0].length;
      continue;
    }

    const identifier = rest.match(/^[A-Za-z_$][A-Za-z0-9_$]*/);
    if (identifier) {
      const token = identifier[0];
      const previous = code[index - 1] ?? '';
      const next = code.slice(index + token.length);
      if (keywords.has(token.toLowerCase())) output += syntaxToken(token, 'keyword');
      else if (CODE_BUILTINS.has(token)) output += syntaxToken(token, 'builtin');
      else if (previous === '.') output += syntaxToken(token, 'property');
      else if (/^\s*[:<]/.test(next) && /^[A-Z]/.test(token)) output += syntaxToken(token, 'type');
      else if (/^[A-Z][A-Za-z0-9_]*$/.test(token)) output += syntaxToken(token, 'type');
      else if (/^\s*\(/.test(next)) output += syntaxToken(token, 'function');
      else output += escapeHtml(token);
      index += token.length;
      continue;
    }

    const operator = rest.match(/^(?:=>|->|::|\.\.|\.\.=|===|!==|==|!=|<=|>=|&&|\|\||\+\+|--|[+\-*\/%=!<>&|^~?:]+)/);
    if (operator) {
      output += syntaxToken(operator[0], 'operator');
      index += operator[0].length;
      continue;
    }

    const punctuation = rest.match(/^[{}()[\].,;]/);
    if (punctuation) {
      output += syntaxToken(punctuation[0], 'punctuation');
      index += punctuation[0].length;
      continue;
    }

    output += escapeHtml(code[index]);
    index += 1;
  }
  return output;
}

function highlightDiffCode(code: string): string {
  return code.split(/(\n)/).map((line) => {
    if (line === '\n') return line;
    if (line.startsWith('+++') || line.startsWith('---') || line.startsWith('@@')) return syntaxToken(line, 'diff-meta');
    if (line.startsWith('+')) return syntaxToken(line, 'diff-add');
    if (line.startsWith('-')) return syntaxToken(line, 'diff-delete');
    return escapeHtml(line);
  }).join('');
}

function highlightMarkupCode(code: string): string {
  return code.replace(/(<!--[\s\S]*?-->)|(<\/?)([A-Za-z][A-Za-z0-9:-]*)([^>]*)(>)/g, (...args: any[]) => {
    const [match, comment, open, tag, attrs, close] = args as [string, string | undefined, string | undefined, string | undefined, string | undefined, string | undefined];
    void match;
    if (comment) return syntaxToken(comment, 'comment');
    return `${syntaxToken(open ?? '', 'punctuation')}${syntaxToken(tag ?? '', 'tag')}${highlightMarkupAttrs(attrs ?? '')}${syntaxToken(close ?? '', 'punctuation')}`;
  });
}

function highlightMarkupAttrs(attrs: string): string {
  return attrs.replace(/([A-Za-z_:][-A-Za-z0-9_:.]*)(\s*=\s*)("[^"]*"|'[^']*'|[^\s>]+)/g, (_match, name: string, equals: string, value: string) => (
    `${syntaxToken(name, 'attr')}${syntaxToken(equals, 'operator')}${syntaxToken(value, 'string')}`
  ));
}

function keywordsForLanguage(language: string): Set<string> {
  return new Set([...COMMON_CODE_KEYWORDS, ...(LANGUAGE_CODE_KEYWORDS[language] ?? [])]);
}

function stringLiteralPattern(language: string): RegExp {
  if (['shell', 'bash'].includes(language)) return /^(?:\$?'(?:\\.|[^'\\])*'|\$?"(?:\\.|[^"\\])*"|`(?:\\.|[^`\\])*`)/;
  if (language === 'python') return /^(?:(?:[rbu]|br|rb|fr|rf)?(?:'{3}[\s\S]*?'{3}|"{3}[\s\S]*?"{3}|'(?:\\.|[^'\\])*'|"(?:\\.|[^"\\])*"))/i;
  return /^(?:"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`)/;
}

function lineCommentPattern(language: string): RegExp {
  if (['python', 'shell', 'yaml', 'toml', 'ruby'].includes(language)) return /^#[^\n]*/;
  return /^\/\/[^\n]*/;
}

function syntaxToken(value: string, kind: SyntaxKind): string {
  return `<span class="vv-syntax-${kind}">${escapeHtml(value)}</span>`;
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
  const last = state.approvalFeedback;
  const lastResult = last && last.status !== 'pending'
    ? `<div class="vv-panel mb-4 p-4 text-sm ${last.status === 'success' ? 'border-vv-green/45 text-green-100' : last.status === 'error' ? 'border-vv-red/45 text-red-100' : 'border-vv-amber/45 text-amber-100'}">
        <div class="font-black">Last approval result</div>
        <p class="mt-2 leading-6">${escapeHtml(last.message)}</p>
      </div>`
    : '';
  if (!state.approvals.length) return `${lastResult}<div class="vv-panel p-4 text-sm text-vv-muted">No pending approvals. ${last?.status === 'success' ? 'Last approval executed successfully.' : 'The beast is behaving.'}</div>`;
  return `${lastResult}<div class="grid gap-4">${state.approvals.map(renderApprovalCard).join('')}</div>`;
}

function renderApprovalCard(approval: any): string {
  const id = String(approval.id ?? '');
  const feedback = id ? state.approvalActions[id] : undefined;
  const busy = feedback?.status === 'pending';
  const statusLine = feedback
    ? `<div class="mt-3 rounded-xl border border-vv-cyan/35 bg-vv-cyan/10 px-3 py-2 text-xs text-cyan-100"><span class="inline-block h-2 w-2 rounded-full bg-vv-cyan ${busy ? 'animate-pulse' : ''}"></span> ${escapeHtml(feedback.message)}</div>`
    : '';
  return `
    <article class="rounded-[1.05rem] border border-vv-red/45 bg-vv-red/10 p-4 shadow-danger">
      <h3 class="text-base font-black text-red-100">${escapeHtml(approval.tool_name ?? approval.toolName ?? 'approval')}</h3>
      <p class="mt-2 text-sm text-red-100/75">${escapeHtml(approval.reason ?? approval.risk_label ?? 'Risky action requires approval.')}</p>
      <pre class="vv-code mt-3 whitespace-pre-wrap rounded-xl bg-black/20 p-3">${escapeHtml(JSON.stringify(approval.args ?? {}, null, 2))}</pre>
      ${statusLine}
      <div class="mt-3 flex flex-wrap gap-2">
        <button class="vv-action" data-approval="${escapeHtml(id)}" data-method="approvals.approveOnceAndExecute" ${busy ? 'disabled' : ''}>${busy ? 'Working…' : 'Approve once'}</button>
        <button class="vv-action" data-approval="${escapeHtml(id)}" data-method="approvals.approveSessionAndExecute" ${busy ? 'disabled' : ''}>Approve session</button>
        <button class="vv-action vv-action-danger" data-approval="${escapeHtml(id)}" data-method="approvals.deny" ${busy ? 'disabled' : ''}>Deny</button>
      </div>
    </article>`;
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
          <button class="vv-action" data-bridge-method="bridge.capabilities" ${bridgeMethodDisabledAttr('bridge.capabilities')}>Refresh capabilities</button>
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
  return `<button class="vv-soft-panel w-full p-3 text-left hover:border-vv-line2" data-bridge-method="${escapeHtml(method)}" ${bridgeMethodDisabledAttr(method)}>
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
          <button class="vv-action" data-bridge-method="${escapeHtml(primaryMethod)}" ${bridgeMethodDisabledAttr(primaryMethod)}>Refresh</button>
        </div>
        ${renderBridgeMethodParameterForm(selected)}
        <div class="mt-4 grid gap-2 md:grid-cols-2 xl:grid-cols-3">
          ${actions.map(([method, label, hint]) => `<button class="vv-soft-panel p-3 text-left hover:border-vv-line2 ${selected === method ? 'border-vv-cyan bg-vv-cyan/10' : ''}" data-bridge-method="${escapeHtml(method)}" ${bridgeMethodDisabledAttr(method)}>
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
        <button class="vv-action vv-action-primary" data-run-selected-bridge-method="${escapeHtml(selectedMethod)}" ${bridgeMethodDisabledAttr(selectedMethod)}>Run selected</button>
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


function renderExplorer(): string {
  const explorer = state.fileExplorer;
  const currentWorkspace = String(state.session?.workspace ?? state.settings.workspace ?? '').trim();
  const selectedEntry = explorer.entries.find((entry) => entry.path === explorer.selectedPath);
  const selectedDirectory = selectedEntry?.isDir ? selectedEntry.path : explorer.selectedPath;
  const workspacePath = selectedDirectory || explorer.path || currentWorkspace;
  return `
    <div class="grid h-full min-h-0 grid-rows-[auto_minmax(0,1fr)] gap-3">
      <section class="vv-panel p-3">
        <div class="flex flex-wrap items-start justify-between gap-3">
          <div class="min-w-0 flex-1">
            <h2 class="text-base font-black">Workspace Explorer</h2>
            <p class="mt-1 text-xs leading-5 text-vv-muted">Browse local directories and switch the Vegvisir bridge workspace with the mouse.</p>
            <div class="mt-2 flex flex-wrap items-center gap-2 text-xs text-vv-muted">
              <span class="vv-pill">active: ${escapeHtml(currentWorkspace || 'default/home')}</span>
              <span class="vv-pill text-vv-cyan ${explorer.loading ? '' : 'hidden'}" data-explorer-loading-badge><span class="vv-mini-spinner" aria-hidden="true"></span> loading</span>
            </div>
          </div>
          <div class="flex flex-wrap gap-2">
            <button class="vv-action" id="explorer-home" ${explorer.home ? '' : 'disabled'}>Home</button>
            <button class="vv-action" id="explorer-current-workspace" ${currentWorkspace ? '' : 'disabled'}>Current workspace</button>
            <button class="vv-action" id="explorer-up" ${explorer.parent ? '' : 'disabled'}>Up</button>
            <button class="vv-action" id="explorer-refresh">Refresh</button>
          </div>
        </div>
        <div class="mt-3 flex gap-2">
          <input id="explorer-path-input" class="vv-focus min-w-0 flex-1 rounded-xl border border-vv-line bg-black/20 px-3 py-2 font-mono text-xs text-vv-text placeholder:text-vv-dim" value="${escapeHtml(explorer.path || currentWorkspace)}" placeholder="Directory path" />
          <button class="vv-action" id="explorer-go">Go</button>
          <button class="vv-action vv-action-primary" data-explorer-switch-workspace="${escapeHtml(workspacePath)}" ${workspacePath ? '' : 'disabled'}>${state.bridgeRunning ? 'Switch + restart' : 'Set workspace'}</button>
        </div>
        ${explorer.error ? `<pre class="vv-code mt-3 whitespace-pre-wrap rounded-xl border border-vv-red/45 bg-vv-red/10 p-3 text-red-100">${escapeHtml(explorer.error)}</pre>` : ''}
        ${renderExplorerBreadcrumbs(explorer.path)}
      </section>
      <section class="vv-panel min-h-0 overflow-hidden p-0">
        <div class="grid grid-cols-[minmax(0,1fr)_auto_auto_auto] gap-2 border-b border-vv-line bg-black/20 px-3 py-2 font-mono text-[0.68rem] uppercase tracking-[0.18em] text-vv-muted">
          <span>Name</span><span>Size</span><span>Modified</span><span>Action</span>
        </div>
        <div class="vv-scrollbar h-full min-h-0 overflow-auto" data-explorer-list>
          ${renderExplorerEntries()}
        </div>
      </section>
    </div>`;
}

function renderExplorerEntries(): string {
  const explorer = state.fileExplorer;
  if (!explorer.entries.length) {
    return `<div class="p-4 text-sm text-vv-muted">${explorer.loading ? 'Loading directory…' : 'No entries loaded. Click Home, Current workspace, or Go.'}</div>`;
  }
  const visibleLimit = Math.max(1, explorer.visibleLimit || EXPLORER_INITIAL_VISIBLE_ENTRIES);
  const visibleEntries = explorer.entries.slice(0, visibleLimit);
  const hiddenCount = Math.max(0, explorer.entries.length - visibleEntries.length);
  const backendTruncated = explorer.truncated
    ? `<div class="border-b border-vv-line/60 bg-vv-amber/10 px-3 py-2 text-xs text-vv-amber">Showing first ${explorer.entries.length.toLocaleString()} entries returned by the native browser limit. Narrow the path if this directory is larger.</div>`
    : '';
  const showMore = hiddenCount > 0
    ? `<button class="w-full border-b border-vv-line/60 px-3 py-3 text-left text-xs font-semibold text-vv-cyan hover:bg-white/[0.035]" data-explorer-show-more="1">Show ${Math.min(EXPLORER_VISIBLE_INCREMENT, hiddenCount).toLocaleString()} more entries (${hiddenCount.toLocaleString()} hidden)</button>`
    : '';
  return `${backendTruncated}${visibleEntries.map(renderExplorerEntry).join('')}${showMore}`;
}

function renderExplorerBreadcrumbs(path: string): string {
  if (!path) return '';
  const parts = path.split('/').filter(Boolean);
  const rootButton = `<button class="vv-action px-2 py-1 text-[0.68rem]" data-explorer-open="/">/</button>`;
  let cursor = '';
  const buttons = parts.map((part) => {
    cursor += `/${part}`;
    return `<button class="vv-action px-2 py-1 text-[0.68rem]" data-explorer-open="${escapeHtml(cursor)}">${escapeHtml(part)}</button>`;
  });
  return `<div class="vv-scrollbar mt-3 flex items-center gap-1 overflow-x-auto pb-1">${[rootButton, ...buttons].join('<span class="text-vv-dim">›</span>')}</div>`;
}

function renderExplorerEntry(entry: FileExplorerEntry): string {
  const selected = state.fileExplorer.selectedPath === entry.path;
  const icon = entry.isDir ? (entry.gitRepo ? '⌬' : '▸') : '·';
  const typeLabel = entry.isDir ? (entry.gitRepo ? 'git workspace' : 'directory') : entry.isSymlink ? 'symlink' : 'file';
  return `
    <div class="grid grid-cols-[minmax(0,1fr)_auto_auto_auto] items-center gap-2 border-b border-vv-line/60 px-3 py-2 text-sm ${selected ? 'bg-vv-cyan/10' : 'hover:bg-white/[0.035]'}">
      <button class="min-w-0 text-left" data-explorer-select="${escapeHtml(entry.path)}" ${entry.isDir ? `data-explorer-open="${escapeHtml(entry.path)}"` : ''}>
        <div class="flex min-w-0 items-center gap-2">
          <span class="grid h-6 w-6 shrink-0 place-items-center rounded-lg border border-vv-line bg-white/[0.035] text-vv-cyan">${icon}</span>
          <span class="truncate font-semibold text-vv-text">${escapeHtml(entry.name)}</span>
          <span class="vv-pill shrink-0 ${entry.gitRepo ? 'text-vv-green' : ''}">${escapeHtml(typeLabel)}</span>
        </div>
        <div class="mt-1 truncate font-mono text-[0.68rem] text-vv-dim">${escapeHtml(entry.path)}</div>
      </button>
      <span class="font-mono text-xs text-vv-muted">${escapeHtml(formatFileSize(entry.size))}</span>
      <span class="font-mono text-xs text-vv-muted">${escapeHtml(formatModifiedTime(entry.modifiedMs))}</span>
      <div class="flex justify-end gap-1">
        ${entry.isDir ? `<button class="vv-action px-2 py-1 text-[0.65rem]" data-explorer-open="${escapeHtml(entry.path)}">Open</button><button class="vv-action vv-action-primary px-2 py-1 text-[0.65rem]" data-explorer-switch-workspace="${escapeHtml(entry.path)}">Workspace</button>` : ''}
      </div>
    </div>`;
}

function formatFileSize(value: unknown): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) return '—';
  if (value < 1024) return `${value} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let size = value / 1024;
  let index = 0;
  while (size >= 1024 && index < units.length - 1) {
    size /= 1024;
    index += 1;
  }
  return `${size.toFixed(size >= 10 ? 1 : 2)} ${units[index]}`;
}

function formatModifiedTime(value: unknown): string {
  const numeric = typeof value === 'string' ? Number(value) : value;
  if (typeof numeric !== 'number' || !Number.isFinite(numeric)) return '—';
  const date = new Date(numeric);
  if (Number.isNaN(date.getTime())) return '—';
  return date.toLocaleDateString();
}

async function loadExplorerDirectory(path?: string): Promise<void> {
  const token = state.fileExplorer.requestToken + 1;
  state.fileExplorer = {
    ...state.fileExplorer,
    loading: true,
    error: '',
    requestToken: token,
  };
  updateExplorerLoadingIndicator();
  try {
    const listing = await invoke<FileExplorerListing>('fs_list_directory', { path: path || null });
    if (state.fileExplorer.requestToken !== token) return;
    state.fileExplorer = {
      path: listing.path,
      parent: listing.parent ?? '',
      home: listing.home ?? '',
      entries: Array.isArray(listing.entries) ? listing.entries : [],
      loading: false,
      error: '',
      selectedPath: listing.path,
      visibleLimit: EXPLORER_INITIAL_VISIBLE_ENTRIES,
      truncated: Boolean(listing.truncated),
      totalEntries: Number(listing.totalEntries ?? listing.entries?.length ?? 0),
      requestToken: token,
    };
  } catch (error) {
    if (state.fileExplorer.requestToken !== token) return;
    state.fileExplorer = {
      ...state.fileExplorer,
      loading: false,
      error: String(error),
      requestToken: token,
    };
  }
  render();
}

function updateExplorerLoadingIndicator(): void {
  const explorerRoots = document.querySelectorAll('[data-panel-root="explorer"]');
  if (!explorerRoots.length) {
    render();
    return;
  }
  explorerRoots.forEach((explorerRoot) => {
    const badge = explorerRoot.querySelector<HTMLElement>('[data-explorer-loading-badge]');
    if (badge) badge.classList.remove('hidden');
  });
}

async function switchWorkspace(path: string): Promise<void> {
  const workspace = path.trim();
  if (!workspace) return;
  state.settings.workspace = workspace;
  saveSettings();
  state.fileExplorer.selectedPath = workspace;
  state.activePanel = 'explorer';
  state.events.push({ type: 'desktop.workspace.selected', payload: { workspace } });
  if (state.bridgeRunning) {
    await restartBridge();
  } else {
    render();
  }
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
      <label class="flex items-center gap-3 text-sm text-vv-muted"><input type="checkbox" name="dangerousBypass" ${startupDangerousBypassRequested() ? 'checked' : ''} /> Dangerous bypass at startup</label>
      <p class="text-xs leading-5 text-vv-muted">Packaged AppImages may not inherit your shell PATH. If bridge start fails, set the Vegvisir binary to an absolute path such as <code class="text-vv-cyan">/home/malice/.local/bin/vegvisir</code>. The backend also searches resource-adjacent <code class="text-vv-cyan">resources/bin</code> paths and <code class="text-vv-cyan">VEGVISIR_DESKTOP_RESOURCE_DIR</code> for future bundled binaries.</p>
      <p class="text-xs leading-5 text-vv-muted">Desktop does not bypass Vegvisir. It spawns <code class="text-vv-cyan">vegvisir app-server</code> so providers, HBSE, CMS, tools, approvals, and policy remain owned by the harness.</p>
      <div class="flex flex-wrap gap-2"><button class="vv-action vv-action-primary w-fit" type="submit">Save settings</button><button class="vv-action w-fit" type="button" data-module-add="explorer">Open Explorer</button></div>
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
  if (state.activePanel === 'chat') return isApprovalPaused() ? 'Vegvisir is paused for approval' : state.busy ? 'Vegvisir is working…' : 'Ask Vegvisir to work';
  return label;
}

function projectName(): string {
  const workspace = state.session?.workspace ?? state.settings.workspace ?? '';
  const trimmed = String(workspace).replace(/\/$/, '');
  if (!trimmed) return 'local';
  return trimmed.split('/').filter(Boolean).pop() ?? trimmed;
}


function addOrFocusModule(panelId: PanelId): void {
  const layout = currentLayout();
  const existing = layout.modules.find((module) => module.panelId === panelId);
  if (existing) {
    focusModule(existing.id);
    return;
  }
  addModule(panelId);
}

function addModule(panelId: PanelId): void {
  const layout = currentLayout();
  const panel = panelDefinition(panelId);
  const offset = layout.modules.length % 10;
  const module: CanvasModuleInstance = {
    id: uniqueId(`module-${panelId}`),
    panelId,
    title: panel.label,
    x: snap(48 + offset * 36),
    y: snap(48 + offset * 32),
    width: panel.defaultWidth,
    height: panel.defaultHeight,
    collapsed: false,
    locked: false,
    z: maxModuleZ(layout) + 1,
  };
  layout.modules.push(module);
  state.activePanel = panelId;
  saveLayouts();
  refreshPanelData(panelId);
  render();
}

function removeModule(instanceId: string): void {
  const layout = currentLayout();
  layout.modules = layout.modules.filter((module) => module.id !== instanceId);
  saveLayouts();
  render();
}

function focusModule(instanceId: string): void {
  const layout = currentLayout();
  const module = layout.modules.find((item) => item.id === instanceId);
  if (!module) return;
  module.z = maxModuleZ(layout) + 1;
  state.activePanel = module.panelId;
  saveLayouts();
  refreshPanelData(module.panelId);
  render();
}

function collapseModule(instanceId: string): void {
  const module = currentLayout().modules.find((item) => item.id === instanceId);
  if (!module) return;
  module.collapsed = !module.collapsed;
  module.z = maxModuleZ() + 1;
  saveLayouts();
  render();
}

function toggleModuleLock(instanceId: string): void {
  const module = currentLayout().modules.find((item) => item.id === instanceId);
  if (!module) return;
  module.locked = !module.locked;
  saveLayouts();
  render();
}

function updateModuleBounds(instanceId: string, x: number, y: number, width: number, height: number): void {
  const module = currentLayout().modules.find((item) => item.id === instanceId);
  if (!module) return;
  const panel = panelDefinition(module.panelId);
  module.x = Math.max(0, snap(x));
  module.y = Math.max(0, snap(y));
  module.width = Math.max(panel.minWidth, snap(width));
  module.height = Math.max(panel.minHeight, snap(height));
}

function createLayoutTab(): void {
  const name = window.prompt('Layout tab name?', 'New Layout')?.trim() || 'New Layout';
  const tab: CanvasLayoutTab = { id: uniqueId('layout'), name, modules: [] };
  state.layout.tabs.push(tab);
  state.layout.activeTabId = tab.id;
  saveLayouts();
  render();
}

function duplicateLayoutTab(): void {
  const layout = currentLayout();
  const name = window.prompt('Duplicate layout as?', `${layout.name} Copy`)?.trim() || `${layout.name} Copy`;
  const tab: CanvasLayoutTab = {
    id: uniqueId('layout'),
    name,
    modules: layout.modules.map((module) => ({ ...module, id: uniqueId(`module-${module.panelId}`), x: module.x + 24, y: module.y + 24 })),
  };
  state.layout.tabs.push(tab);
  state.layout.activeTabId = tab.id;
  saveLayouts();
  render();
}

function renameLayoutTab(): void {
  const layout = currentLayout();
  const name = window.prompt('Rename layout tab?', layout.name)?.trim();
  if (!name) return;
  layout.name = name;
  saveLayouts();
  render();
}

function deleteLayoutTab(): void {
  if (state.layout.tabs.length <= 1) return;
  const layout = currentLayout();
  if (!window.confirm(`Delete layout tab "${layout.name}"?`)) return;
  state.layout.tabs = state.layout.tabs.filter((tab) => tab.id !== layout.id);
  state.layout.activeTabId = state.layout.tabs[0]?.id ?? '';
  saveLayouts();
  render();
}

function switchLayoutTab(tabId: string): void {
  if (!state.layout.tabs.some((tab) => tab.id === tabId)) return;
  state.layout.activeTabId = tabId;
  canvasInteraction = null;
  saveLayouts();
  render();
}

function resetLayoutTab(): void {
  const current = currentLayout();
  if (!window.confirm(`Reset layout tab "${current.name}" to its default module set?`)) return;
  const defaults = defaultLayoutState();
  const replacement = defaults.tabs.find((tab) => tab.id === current.id) ?? createLayoutTabFromPanels(current.id, current.name, []);
  current.modules = replacement.modules.map((module) => ({ ...module, id: uniqueId(`module-${module.panelId}`) }));
  saveLayouts();
  render();
}

function refreshPanelData(panel: PanelId): void {
  if (panel === 'explorer') void loadExplorerDirectory(state.fileExplorer.path || state.settings.workspace || undefined);
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
}

function startCanvasInteraction(event: PointerEvent, id: string, mode: 'move' | 'resize'): void {
  if ((event.target as HTMLElement).closest('button')) return;
  const module = currentLayout().modules.find((item) => item.id === id);
  if (!module || module.locked) return;
  event.preventDefault();
  module.z = maxModuleZ() + 1;
  canvasInteraction = {
    mode,
    id,
    startClientX: event.clientX,
    startClientY: event.clientY,
    startX: module.x,
    startY: module.y,
    startWidth: module.width,
    startHeight: module.height,
  };
  render();
}

function updateCanvasInteraction(event: PointerEvent): void {
  const interaction = canvasInteraction;
  if (!interaction) return;
  const module = currentLayout().modules.find((item) => item.id === interaction.id);
  if (!module) return;
  const deltaX = event.clientX - interaction.startClientX;
  const deltaY = event.clientY - interaction.startClientY;
  if (interaction.mode === 'move') {
    updateModuleBounds(interaction.id, interaction.startX + deltaX, interaction.startY + deltaY, module.width, module.height);
  } else {
    updateModuleBounds(interaction.id, module.x, module.y, interaction.startWidth + deltaX, interaction.startHeight + deltaY);
  }
  applyCanvasInteractionDomUpdate(module);
}

function applyCanvasInteractionDomUpdate(module: CanvasModuleInstance): void {
  const frame = document.querySelector<HTMLElement>(`[data-module-frame="${cssEscape(module.id)}"]`);
  if (frame) {
    frame.style.left = `${module.x}px`;
    frame.style.top = `${module.y}px`;
    frame.style.width = `${module.width}px`;
    frame.style.height = `${module.height}px`;
  }
  const canvasContent = document.querySelector<HTMLElement>('#canvas-content');
  if (canvasContent) {
    const bounds = canvasBounds(currentLayout().modules);
    canvasContent.style.width = `${bounds.width}px`;
    canvasContent.style.height = `${bounds.height}px`;
  }
}

function finishCanvasInteraction(): void {
  if (!canvasInteraction) return;
  canvasInteraction = null;
  saveLayouts();
  render();
}

function handleExplorerClick(event: Event): void {
  const target = event.target as HTMLElement | null;
  const showMore = target?.closest<HTMLElement>('[data-explorer-show-more]');
  if (showMore) {
    event.preventDefault();
    state.fileExplorer.visibleLimit += EXPLORER_VISIBLE_INCREMENT;
    render();
    return;
  }
  const switchButton = target?.closest<HTMLElement>('[data-explorer-switch-workspace]');
  if (switchButton) {
    event.preventDefault();
    event.stopPropagation();
    void switchWorkspace(switchButton.dataset.explorerSwitchWorkspace ?? '');
    return;
  }
  const openButton = target?.closest<HTMLElement>('[data-explorer-open]');
  if (openButton) {
    event.preventDefault();
    event.stopPropagation();
    void loadExplorerDirectory(openButton.dataset.explorerOpen ?? '');
    return;
  }
  const selectButton = target?.closest<HTMLElement>('[data-explorer-select]');
  if (selectButton) {
    event.preventDefault();
    state.fileExplorer.selectedPath = selectButton.dataset.explorerSelect ?? '';
    render();
  }
}

function bindEvents(): void {
  document.querySelector('#start-stop')?.addEventListener('click', () => state.bridgeRunning ? void stopBridge() : void startBridge());
  document.querySelector('#restart-bridge')?.addEventListener('click', () => void restartBridge());
  document.querySelector('#restart-bridge-from-error')?.addEventListener('click', () => void restartBridge());
  document.querySelector('#clear-approval-feedback')?.addEventListener('click', () => { state.approvalFeedback = null; render(); });
  document.querySelector('#refresh-all')?.addEventListener('click', () => void refreshEverything());
  document.querySelectorAll<HTMLButtonElement>('[data-panel]').forEach((button) => button.addEventListener('click', () => setPanel(button.dataset.panel ?? 'chat')));
  document.querySelectorAll<HTMLButtonElement>('[data-module-add]').forEach((button) => button.addEventListener('click', () => {
    const panelId = button.dataset.moduleAdd;
    if (isPanelId(panelId)) addModule(panelId);
  }));
  document.querySelectorAll<HTMLButtonElement>('[data-module-remove]').forEach((button) => button.addEventListener('click', () => removeModule(button.dataset.moduleRemove ?? '')));
  document.querySelectorAll<HTMLButtonElement>('[data-module-focus]').forEach((button) => button.addEventListener('click', () => focusModule(button.dataset.moduleFocus ?? '')));
  document.querySelectorAll<HTMLButtonElement>('[data-module-collapse]').forEach((button) => button.addEventListener('click', () => collapseModule(button.dataset.moduleCollapse ?? '')));
  document.querySelectorAll<HTMLButtonElement>('[data-module-lock]').forEach((button) => button.addEventListener('click', () => toggleModuleLock(button.dataset.moduleLock ?? '')));
  document.querySelectorAll<HTMLButtonElement>('[data-module-open-full]').forEach((button) => button.addEventListener('click', () => {
    const panelId = button.dataset.moduleOpenFull;
    if (isPanelId(panelId)) addOrFocusModule(panelId);
  }));
  document.querySelectorAll<HTMLButtonElement>('[data-layout-tab]').forEach((button) => button.addEventListener('click', () => switchLayoutTab(button.dataset.layoutTab ?? '')));
  document.querySelector('#layout-add-tab')?.addEventListener('click', () => createLayoutTab());
  document.querySelector('#layout-duplicate-tab')?.addEventListener('click', () => duplicateLayoutTab());
  document.querySelector('#layout-rename-tab')?.addEventListener('click', () => renameLayoutTab());
  document.querySelector('#layout-delete-tab')?.addEventListener('click', () => deleteLayoutTab());
  document.querySelector('#layout-reset-tab')?.addEventListener('click', () => resetLayoutTab());
  document.querySelector('#layout-save')?.addEventListener('click', () => saveLayouts());
  document.querySelector('#layout-snap')?.addEventListener('change', (event) => {
    state.layout.snapToGrid = (event.currentTarget as HTMLInputElement).checked;
    saveLayouts();
    render();
  });
  document.querySelectorAll<HTMLElement>('[data-module-drag]').forEach((handle) => handle.addEventListener('pointerdown', (event) => startCanvasInteraction(event, handle.dataset.moduleDrag ?? '', 'move')));
  document.querySelectorAll<HTMLElement>('[data-module-resize]').forEach((handle) => handle.addEventListener('pointerdown', (event) => startCanvasInteraction(event, handle.dataset.moduleResize ?? '', 'resize')));
  document.querySelector('#explorer-home')?.addEventListener('click', () => void loadExplorerDirectory(state.fileExplorer.home || undefined));
  document.querySelector('#explorer-current-workspace')?.addEventListener('click', () => void loadExplorerDirectory(String(state.session?.workspace ?? state.settings.workspace ?? '')));
  document.querySelector('#explorer-up')?.addEventListener('click', () => void loadExplorerDirectory(state.fileExplorer.parent));
  document.querySelector('#explorer-refresh')?.addEventListener('click', () => void loadExplorerDirectory(state.fileExplorer.path || state.settings.workspace || undefined));
  document.querySelector('#explorer-go')?.addEventListener('click', () => {
    const input = document.querySelector<HTMLInputElement>('#explorer-path-input');
    void loadExplorerDirectory(input?.value.trim() || undefined);
  });
  document.querySelector('#explorer-path-input')?.addEventListener('keydown', (event) => {
    const key = event as KeyboardEvent;
    if (key.key === 'Enter') {
      key.preventDefault();
      void loadExplorerDirectory((key.currentTarget as HTMLInputElement).value.trim() || undefined);
    }
  });
  document.querySelectorAll('[data-panel-root="explorer"]').forEach((root) => root.addEventListener('click', handleExplorerClick));
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
      settingsSchemaVersion: SETTINGS_SCHEMA_VERSION,
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

function cssEscape(value: string): string {
  if (typeof CSS !== 'undefined' && typeof CSS.escape === 'function') return CSS.escape(value);
  return String(value).replace(/[^a-zA-Z0-9_-]/g, (character) => `\\${character}`);
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

window.addEventListener('pointermove', (event) => updateCanvasInteraction(event));
window.addEventListener('pointerup', () => finishCanvasInteraction());
window.addEventListener('pointercancel', () => finishCanvasInteraction());

void bootstrap();
setInterval(() => void pollBridge(), 350);

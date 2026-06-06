export type BridgeId = string | number;

export type BridgeEvent = {
  type: string;
  id?: BridgeId | null;
  payload?: any;
};

export type BridgeRequest = {
  id: BridgeId;
  method: string;
  params?: Record<string, unknown>;
};

export type Message = {
  role?: string;
  content?: string;
  text?: string;
  timestamp?: string;
};

export type ApprovalFeedback = {
  id: string;
  status: 'pending' | 'success' | 'error' | 'denied';
  message: string;
  method: string;
  updatedAt: number;
};

export type StartBridgeRequest = {
  workspace?: string;
  provider?: string;
  model?: string;
  agent?: string;
  vegvisirBinary?: string;
  dangerousBypass?: boolean;
  autoStart?: boolean;
  settingsSchemaVersion?: number;
};

export type BridgeStatus = {
  running: boolean;
  pid?: number;
};

export type BridgeStopResult = {
  wasRunning: boolean;
  graceful: boolean;
  killed: boolean;
  status?: string | null;
};

export type FileExplorerEntry = {
  name: string;
  path: string;
  isDir: boolean;
  isFile: boolean;
  isSymlink: boolean;
  size?: number | null;
  modifiedMs?: number | string | null;
  gitRepo: boolean;
};

export type FileExplorerListing = {
  path: string;
  parent?: string | null;
  home?: string | null;
  entries: FileExplorerEntry[];
  truncated?: boolean;
  totalEntries?: number;
  limit?: number;
};

export type FileExplorerState = {
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

export type BridgeMethodDraft = {
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

export type PanelId = 'explorer' | 'chat' | 'sessions' | 'work' | 'approvals' | 'tools' | 'providers' | 'capabilities' | 'commands' | 'runtime' | 'openai' | 'diff' | 'memory' | 'skills' | 'integrations' | 'evidence' | 'system' | 'settings';

export type CanvasPanelId = PanelId;

export type PanelDefinition = {
  id: PanelId;
  label: string;
  icon: string;
  hint: string;
  defaultWidth: number;
  defaultHeight: number;
  minWidth: number;
  minHeight: number;
};

export type CanvasModuleInstance = {
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

export type CanvasLayoutTab = {
  id: string;
  name: string;
  modules: CanvasModuleInstance[];
};

export type CanvasLayoutState = {
  schemaVersion: 1;
  activeTabId: string;
  tabs: CanvasLayoutTab[];
  editMode: boolean;
  gridSize: number;
  snapToGrid: boolean;
};

export type CanvasInteraction = {
  mode: 'move' | 'resize';
  id: string;
  startClientX: number;
  startClientY: number;
  startX: number;
  startY: number;
  startWidth: number;
  startHeight: number;
};

export type ChatScrollSnapshot = {
  scrollTop: number;
  bottomOffset: number;
  shouldStickToBottom: boolean;
};

export type CanvasScrollSnapshot = {
  scrollLeft: number;
  scrollTop: number;
  tabId: string;
};

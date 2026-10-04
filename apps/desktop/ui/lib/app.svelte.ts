// Central UI state (Svelte 5 runes). Components read from `app` and call
// the action functions below; all durable state lives in the Rust backend.

import { call, chooseFolder, listen, native, onEngine, onRunUpdate, readable } from './api';
import type {
  ApprovalRequest,
  Combo,
  Conversation,
  EngineEvent,
  GoalSummary,
  HistoryEntry,
  Mode,
  ModelProfile,
  Prefs,
  Project,
  Provider,
  Settings,
  Snapshot,
} from './types';
import { defaultSettings } from './types';
import { ding } from './sound';

export type View = 'home' | 'chat' | 'goal';
export type SettingsTab =
  | 'general'
  | 'providers'
  | 'usage'
  | 'combos'
  | 'permissions'
  | 'optimization'
  | 'instructions'
  | 'integrations'
  | 'remote'
  | 'appearance'
  | 'updates'
  | 'privacy'
  | 'environment'
  | 'about';

export interface Toast {
  id: number;
  text: string;
  kind: 'info' | 'error' | 'success';
  action?: { label: string; run: () => void };
}

export interface ActivityLine {
  at: number;
  kind: 'state' | 'task' | 'tool' | 'notice' | 'check' | 'handoff' | 'done' | 'blocked' | 'effort';
  text: string;
  ok?: boolean;
}

class AppState {
  ready = $state(false);
  native = native;
  settings = $state<Settings>({ ...defaultSettings });
  prefs = $state<Prefs | null>(null);
  projects = $state<Project[]>([]);
  providers = $state<Provider[]>([]);
  conversations = $state<Conversation[]>([]);
  history = $state<HistoryEntry[]>([]);
  dataDir = $state('');
  models = $state<ModelProfile[]>([]);
  combos = $state<Combo[]>([]);
  defaultCombo = $state<string | null>(null);
  goals = $state<GoalSummary[]>([]);

  projectId = $state<string | null>(null);
  conversationId = $state<string | null>(null);
  goalId = $state<string | null>(null);
  view = $state<View>('home');

  /** Chat or Agent; Goal and Plan are chosen per message with /goal and /plan. */
  mode = $state<Mode>('chat');
  /** Keeps Goal/Plan active while a slash-command message is being started. */
  forcedMode = $state<Mode | null>(null);
  target = $state<string | null>(null);
  effort = $state<string>('auto');
  draft = $state('');

  running = $state<Record<string, boolean>>({});
  approvals = $state<ApprovalRequest[]>([]);
  activity = $state<Record<string, ActivityLine[]>>({});
  goalVersion = $state(0);
  toasts = $state<Toast[]>([]);
  effortRequest = $state<{ model: string; level: string; why: string } | null>(null);

  sidebarOpen = $state(true);
  settingsTab = $state<SettingsTab | null>(null);
  inspectorOpen = $state(false);
  paletteOpen = $state(false);
  gitOpen = $state(false);
  historyOpen = $state(false);
  memoryOpen = $state(false);
  previewOpen = $state(false);
  wizardOpen = $state(false);
  newProjectOpen = $state(false);
  closePrompt = $state<boolean | null>(null);

  get project(): Project | null {
    return this.projects.find((p) => p.id === this.projectId) ?? null;
  }
  get conversation(): Conversation | null {
    return this.conversations.find((c) => c.id === this.conversationId) ?? null;
  }
  get projectConversations(): Conversation[] {
    return this.conversations
      .filter((c) => c.project_id === this.projectId)
      .sort((a, b) => b.updated_at - a.updated_at);
  }
  get projectGoals(): GoalSummary[] {
    return this.goals.filter((g) => g.project_id === this.projectId);
  }
  get connected(): Provider[] {
    return this.providers.filter((p) => p.enabled);
  }
  get anyRunning(): boolean {
    return (
      Object.values(this.running).some(Boolean) ||
      this.goals.some((g) => ['planning', 'running', 'verifying'].includes(g.state))
    );
  }
}

export const app = new AppState();

let toastId = 0;
/** The slash command at the start of the prompt, if any. */
export function slashCommand(text: string): 'goal' | 'plan' | null {
  const m = /^\/(goal|plan)(?=\s|$)/i.exec(text);
  return m ? (m[1].toLowerCase() as 'goal' | 'plan') : null;
}

/** The mode the current prompt will run in. */
export function draftMode(): Mode {
  return app.forcedMode ?? slashCommand(app.draft) ?? (app.mode === 'agent' ? 'agent' : 'chat');
}

export function toast(text: string, kind: Toast['kind'] = 'info', action?: Toast['action']) {
  const t = { id: ++toastId, text, kind, action };
  app.toasts = [...app.toasts, t];
  setTimeout(() => dismissToast(t.id), kind === 'error' ? 8000 : 4500);
}
export function dismissToast(idv: number) {
  app.toasts = app.toasts.filter((t) => t.id !== idv);
}

/** Run work, toast errors, and report success as a boolean. */
export async function succeeded(work: () => Promise<unknown>, success?: string): Promise<boolean> {
  try {
    await work();
    if (success) toast(success, 'success');
    return true;
  } catch (e) {
    toast(readable(e), 'error');
    return false;
  }
}

export async function attempt<T>(work: () => Promise<T>, success?: string): Promise<T | undefined> {
  try {
    const r = await work();
    if (success) toast(success, 'success');
    return r;
  } catch (e) {
    toast(readable(e), 'error');
    return undefined;
  }
}

function applySnapshot(s: Snapshot) {
  app.settings = { ...defaultSettings, ...s.settings };
  app.projects = s.projects.sort((a, b) => b.opened_at - a.opened_at);
  app.providers = s.providers;
  app.conversations = s.conversations;
  app.history = s.history.sort((a, b) => b.created_at - a.created_at);
  app.dataDir = s.data_dir;
}

export type Panel = 'inspector' | 'git' | 'history' | 'memory' | 'preview';
/** Side panels are exclusive so they never crowd the work area. */
export function togglePanel(p: Panel) {
  const open = {
    inspector: app.inspectorOpen,
    git: app.gitOpen,
    history: app.historyOpen,
    memory: app.memoryOpen,
    preview: app.previewOpen,
  }[p];
  app.inspectorOpen = app.gitOpen = app.historyOpen = app.memoryOpen = app.previewOpen = false;
  if (!open) {
    if (p === 'inspector') app.inspectorOpen = true;
    if (p === 'git') app.gitOpen = true;
    if (p === 'history') app.historyOpen = true;
    if (p === 'memory') app.memoryOpen = true;
    if (p === 'preview') app.previewOpen = true;
  }
}

export async function refresh() {
  const [s, models, combos, goals] = await Promise.all([
    call<Snapshot>('snapshot'),
    call<ModelProfile[]>('models'),
    call<{ combos: Combo[]; default_combo: string | null }>('combos_list'),
    call<GoalSummary[]>('goals_list'),
  ]);
  applySnapshot(s);
  app.models = models;
  app.combos = combos.combos;
  app.defaultCombo = combos.default_combo;
  app.goals = goals;
  ensureTarget();
}

export async function refreshGoals() {
  app.goals = await call<GoalSummary[]>('goals_list');
}

/** Pick a sensible default model/Combo when none (or a stale one) is set. */
export function ensureTarget() {
  const valid = (t: string | null) =>
    !!t &&
    ((t.startsWith('model:') && app.models.some((m) => `model:${m.provider}/${m.model}` === t)) ||
      (t.startsWith('combo:') && app.combos.some((c) => `combo:${c.id}` === t)));
  if (valid(app.target)) return;
  const candidates = [app.prefs?.default_target ?? null, ...(app.prefs?.recents ?? [])];
  for (const c of candidates) {
    if (valid(c)) {
      app.target = c;
      return;
    }
  }
  const preferLocal = app.prefs?.privacy === 'local_first';
  const first = (preferLocal && app.models.find((m) => m.local)) || app.models[0];
  app.target = first ? `model:${first.provider}/${first.model}` : null;
}

export function applyAppearance() {
  const p = app.prefs;
  const root = document.documentElement;
  if (!p) return;
  if (p.theme_mode === 'system') root.removeAttribute('data-theme');
  else root.setAttribute('data-theme', p.theme_mode);
  const minimal = p.reduced_motion || app.settings.performance === 'potato';
  root.setAttribute('data-motion', minimal ? 'minimal' : 'normal');
  root.style.fontSize = `${Math.round(14 * (p.ui_scale || 1))}px`;
  root.style.setProperty('--font-size', `${Math.round(14 * (p.ui_scale || 1))}px`);
}

export async function applyTheme() {
  const name = app.prefs?.theme_name;
  const style =
    document.getElementById('installed-theme') ??
    Object.assign(document.createElement('style'), { id: 'installed-theme' });
  document.head.appendChild(style);
  if (!name) {
    style.textContent = '';
    return;
  }
  const themes = await call<{ name: string; light: Record<string, string>; dark: Record<string, string> }[]>(
    'themes_list',
  ).catch(() => []);
  const t = themes.find((x) => x.name === name);
  if (!t) {
    style.textContent = '';
    return;
  }
  // Values were validated by the backend (no url(), no scripts).
  const vars = (m: Record<string, string>) =>
    Object.entries(m)
      .filter(([k, v]) => /^[a-z-]+$/.test(k) && !/[;{}<>]/.test(v))
      .map(([k, v]) => `--${k}: ${v};`)
      .join(' ');
  style.textContent = `:root { ${vars(t.light)} } @media (prefers-color-scheme: dark) { :root:not([data-theme='light']) { ${vars(t.dark)} } } :root[data-theme='dark'] { ${vars(t.dark)} }`;
}

export async function savePrefs(patch: Partial<Prefs>) {
  if (!app.prefs) return;
  const next = { ...app.prefs, ...patch };
  await call('prefs_save', { prefs: next });
  app.prefs = next;
  applyAppearance();
}

export async function saveSettings(patch: Partial<Settings>) {
  const next = { ...app.settings, ...patch };
  await call('save_settings', { settings: next });
  app.settings = next;
  applyAppearance();
}

// ---------- projects & conversations ----------

export async function openProjectDialog() {
  const path = await chooseFolder('Open project');
  if (!path) return;
  await openProjectPath(path);
}

export async function openProjectPath(path: string) {
  const p = await attempt(() => call<Project>('open_project', { path }));
  if (!p) return;
  await refresh();
  selectProject(p.id);
}

export function selectProject(id: string) {
  app.projectId = id;
  app.conversationId = null;
  app.goalId = null;
  app.view = 'home';
}

export async function newConversation(mode: Mode = app.mode === 'goal' ? 'chat' : app.mode) {
  if (!app.projectId) {
    toast('Open a project first.');
    return null;
  }
  const c = await attempt(() => call<Conversation>('create_conversation', { projectId: app.projectId, mode }));
  if (!c) return null;
  app.conversations = [...app.conversations, c];
  app.conversationId = c.id;
  app.goalId = null;
  app.view = 'chat';
  return c;
}

export function openConversation(id: string) {
  const c = app.conversations.find((x) => x.id === id);
  if (!c) return;
  app.projectId = c.project_id;
  app.conversationId = id;
  app.goalId = null;
  app.view = 'chat';
  if (c.mode !== 'goal') app.mode = c.mode;
}

export function openGoal(id: string) {
  const g = app.goals.find((x) => x.id === id);
  if (g) app.projectId = g.project_id;
  app.goalId = id;
  app.conversationId = null;
  app.view = 'goal';
}

export async function send() {
  const text = app.draft.trim();
  if (!text || !app.target) return;
  if (app.mode === 'goal') return; // handled by the Goal composer flow
  let conv = app.conversation;
  if (!conv || app.view !== 'chat') conv = await newConversation(app.mode);
  if (!conv) return;
  const cid = conv.id;
  app.draft = '';
  // Optimistic user message.
  const local = app.conversations.find((c) => c.id === cid);
  if (local) {
    local.messages = [
      ...local.messages,
      {
        id: `local-${Date.now()}`,
        role: 'user',
        text,
        created_at: Date.now() / 1000,
        status: 'complete',
        provider: null,
      },
    ];
    if (local.title === 'New conversation') local.title = text.slice(0, 70);
  }
  app.running = { ...app.running, [cid]: true };
  try {
    await call('send_message', { conversationId: cid, text, target: app.target, effort: app.effort, mode: app.mode });
  } catch (e) {
    toast(readable(e), 'error');
  } finally {
    app.running = { ...app.running, [cid]: false };
    const s = await call<Snapshot>('snapshot');
    applySnapshot(s);
  }
}

export async function stopCurrent() {
  if (app.view === 'goal' && app.goalId) {
    await attempt(() => call('goal_stop', { id: app.goalId }));
    return;
  }
  await attempt(() => call('stop', { conversationId: app.conversationId }));
}

export async function emergencyStop() {
  const r = await attempt(() => call<{ chats: number; goals: number }>('emergency_stop'));
  if (r) toast('Stopped all active work.', 'success');
}

// ---------- events ----------

function pushActivity(goalId: string, line: ActivityLine) {
  const cur = app.activity[goalId] ?? [];
  app.activity = { ...app.activity, [goalId]: [...cur.slice(-300), line] };
}

function onEngineEvent(e: EngineEvent) {
  const now = Date.now();
  switch (e.type) {
    case 'goal': {
      const ev = e.event;
      const add = (kind: ActivityLine['kind'], text: string, ok?: boolean) =>
        pushActivity(e.goal_id, { at: now, kind, text, ok });
      if (ev.type === 'state') {
        if (ev.state !== 'complete') add('state', `Goal ${ev.state.replace(/_/g, ' ')}`);
      } else if (ev.type === 'planned') add('state', `Planned ${ev.tasks} task${ev.tasks === 1 ? '' : 's'}`);
      else if (ev.type === 'task_started')
        add('task', `${ev.title} — ${ev.model}${ev.effort ? ` · ${ev.effort}` : ''}`);
      else if (ev.type === 'task_finished') add('task', `${ev.task}: ${ev.status}`, ev.status === 'done');
      else if (ev.type === 'notice') {
        add('notice', ev.message);
        toast(ev.message);
      } else if (ev.type === 'handoff')
        add('handoff', `Handed off ${ev.from} → ${ev.to} (~${ev.packet_tokens} tokens)`);
      else if (ev.type === 'check') add('check', ev.check, ev.passed);
      else if (ev.type === 'blocked') add('blocked', ev.reason, false);
      else if (ev.type === 'complete') {
        add('done', 'Goal complete — verified', true);
        toast('Goal complete', 'success');
      } else if (ev.type === 'effort_request') app.effortRequest = { model: ev.model, level: ev.level, why: ev.why };
      else if (ev.type === 'reused') add('task', `${ev.task}: reused earlier result from ${ev.from_model}`);
      app.goalVersion++;
      break;
    }
    case 'tool':
      if (e.goal_id) pushActivity(e.goal_id, { at: now, kind: 'tool', text: e.summary, ok: e.ok });
      break;
    case 'approval':
      app.approvals = [
        ...app.approvals.filter((a) => a.id !== e.id),
        { id: e.id, goal_id: e.goal_id, capability: e.capability, detail: e.detail, risk: e.risk },
      ];
      break;
    case 'approval_resolved':
      app.approvals = app.approvals.filter((a) => a.id !== e.id);
      break;
    case 'goal_saved': {
      const g = app.goals.find((x) => x.id === e.goal_id);
      if (g) {
        g.done = e.done;
        g.total = e.total;
        g.state = e.state.replace(/([a-z])([A-Z])/g, '$1_$2').toLowerCase() as GoalSummary['state'];
      } else void refreshGoals();
      app.goalVersion++;
      break;
    }
  }
}

export async function init() {
  try {
    app.prefs = await call<Prefs>('prefs_get');
    await refresh();
  } catch (e) {
    toast(readable(e), 'error');
  }
  applyAppearance();
  void applyTheme();
  if (app.settings.last_project && app.projects.some((p) => p.id === app.settings.last_project)) {
    app.projectId = app.settings.last_project;
  } else if (app.projects.length) {
    app.projectId = app.projects[0].id;
  }
  app.wizardOpen = !app.settings.setup_complete;
  app.ready = true;

  await onRunUpdate((u) => {
    const c = app.conversations.find((x) => x.id === u.conversation_id);
    if (!c) return;
    let m = c.messages.find((x) => x.id === u.message_id);
    if (!m) {
      m = {
        id: u.message_id,
        role: 'assistant',
        text: '',
        created_at: Date.now() / 1000,
        status: 'streaming',
        provider: u.model,
      };
      c.messages = [...c.messages, m];
    }
    m.text = u.text;
    m.status = u.status;
    if (u.notice) toast(u.notice);
  });
  await onEngine(onEngineEvent);
  await listen<{ model: string; level: string; why: string }>('effort-request', (r) => (app.effortRequest = r));
  // OS notifications come from the backend; the ding plays here (Settings › General).
  // Model lists refresh in the background; new models show up right away.
  await listen<number>('providers-changed', () => void refresh());
  await listen<{ title: string; body: string }>('notify', () => {
    if (app.prefs?.notify_sound !== false) ding();
  });
  await listen<{ chats: number; goals: number }>('emergency-stop', () =>
    toast('Emergency stop: all active work halted.', 'success'),
  );
  await listen<string[]>('goals-recovered', (ids) => {
    void refreshGoals();
    toast(`${ids.length} Goal${ids.length === 1 ? ' was' : 's were'} interrupted. Resume from Goals.`, 'info', {
      label: 'Open',
      run: () => openGoal(ids[0]),
    });
  });
  await listen<boolean>('close-requested', (active) => (app.closePrompt = active));
  const pending = await call<ApprovalRequest[]>('approvals_pending').catch(() => []);
  app.approvals = pending;
}

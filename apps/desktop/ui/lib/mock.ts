// Browser-preview backend. All providers, models and numbers here are
// sample data; the desktop app lists models live from each provider.
// In-memory backend used only when the UI runs in a plain browser (UI
// development and automated UI tests). It never touches the network or disk.

import type {
  Combo,
  Conversation,
  GoalRecord,
  Message,
  Prefs,
  Project,
  Provider,
  Settings,
  Snapshot,
  ModelProfile,
  ContextPack,
  QuestionRound,
} from './types';
import { defaultSettings } from './types';

type Listener = (payload: unknown) => void;
const listeners = new Map<string, Set<Listener>>();
function emit(event: string, payload: unknown) {
  listeners.get(event)?.forEach((l) => l(payload));
}
const now = () => Math.floor(Date.now() / 1000);
const id = () => Math.random().toString(36).slice(2, 12);

const prefsDefault: Prefs = {
  close_behavior: 'background',
  theme_mode: 'system',
  theme_name: '',
  reduced_motion: false,
  ui_scale: 1,
  privacy: 'standard',
  telemetry: false,
  update_channel: 'stable',
  auto_update_check: true,
  launch_at_login: false,
  routing: 'balanced',
  max_effort: {},
  provider_instructions: {},
  role_instructions: {},
  keybindings: { palette: 'Mod+K', new_chat: 'Mod+N', open_project: 'Mod+O', settings: 'Mod+,', inspector: 'Mod+I' },
  favorites: [],
  recents: [],
  default_target: null,
  remote_enabled: false,
  remote_bind: '127.0.0.1',
  remote_port: 47820,
  remote_continue_on_disconnect: true,
  emergency_shortcut: 'CmdOrCtrl+Alt+Shift+X',
  subscription_signin: false,
  notify_done: true,
  notify_sound: true,
  notify_low_usage: true,
  auto_update_apps: true,
  seen_resume: {},
};

const db = {
  settings: { ...defaultSettings } as Settings,
  prefs: { ...prefsDefault } as Prefs,
  projects: [] as Project[],
  providers: [] as Provider[],
  conversations: [] as Conversation[],
  history: [] as Snapshot['history'],
  combos: [] as Combo[],
  defaultCombo: null as string | null,
  goals: [] as GoalRecord[],
  mcp: [] as { name: string; enabled: boolean; source: string; transport: unknown; description: string }[],
  host: false,
  pastes: {} as Record<string, string>,
  notified: 0,
  gitInstalled: false,
  signedConnectors: new Set<string>(),
  subscriptions: {} as Record<string, { account: string | null; plan: string | null }>,
};

function profiles(): ModelProfile[] {
  return db.providers
    .filter((p) => p.enabled)
    .flatMap((p) =>
      p.models.map((m) => ({
        provider: p.id,
        model: m.id,
        display: `${p.name} · ${m.name}`,
        tier: /mini|flash|haiku/i.test(m.id) ? 'fast' : /opus|pro/i.test(m.id) ? 'frontier' : 'strong',
        capabilities: ['coding', 'reasoning'],
        efforts: m.efforts,
        context_window: m.context_window ?? 128000,
        local: false,
      })),
    ) as ModelProfile[];
}

function presets(): Combo[] {
  const ps = profiles();
  if (!ps.length) return [];
  const mk = (cid: string, name: string, description: string, members: [string, string[]][]): Combo => ({
    id: cid,
    name,
    description,
    version: 1,
    members: members.map(([model, roles]) => ({
      model,
      roles: roles as Combo['members'][0]['roles'],
      effort: null,
      enabled: true,
      weight: 1,
    })),
    strategy: 'balanced',
    strategy_provider: null,
    reserves: [],
    fallback: true,
    review: 'important',
    max_parallel: 2,
    budget: { max_tokens: null, max_cost_usd: null, ask_at_fraction: null },
    instructions: '',
    phases: [],
    builtin: true,
    history: [],
  });
  const key = (p: ModelProfile) => `${p.provider}/${p.model}`;
  const strong = ps.find((p) => p.tier !== 'fast') ?? ps[0];
  const fast = ps.find((p) => p.tier === 'fast') ?? ps[0];
  const other = ps.find((p) => p.provider !== strong.provider) ?? strong;
  return [
    mk('balanced', 'Balanced', 'Planner, coder and an independent reviewer.', [
      [key(fast), ['planner', 'researcher']],
      [key(strong), ['coder', 'debugger']],
      [key(other), ['reviewer']],
    ]),
    mk('fast', 'Fast', 'Lowest latency model for everything.', [[key(fast), []]]),
    mk('low_usage', 'Low Usage', 'Cheapest model works, strong model reviews important changes.', [
      [key(fast), []],
      [key(strong), ['reviewer']],
    ]),
    mk('maximum_quality', 'Maximum Quality', 'Strongest models with mandatory review.', [
      [key(strong), ['architect', 'coder']],
      [key(other), ['reviewer']],
    ]),
    mk('coding', 'Coding', 'Strong coder, cheap fixer, independent reviewer.', [
      [key(strong), ['coder', 'architect']],
      [key(fast), ['tester', 'documentation']],
      [key(other), ['reviewer']],
    ]),
  ];
}

function snapshot(): Snapshot {
  return {
    settings: db.settings,
    projects: db.projects,
    providers: db.providers,
    conversations: db.conversations,
    history: db.history,
    data_dir: '(browser preview — nothing is saved)',
  };
}

function hist(project_id: string | null, kind: string, summary: string) {
  db.history.push({ id: id(), project_id, kind, summary, created_at: now() });
}

const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function streamReply(
  conversation: Conversation,
  text: string,
  target: string,
  effort: string | null,
  mode: string,
) {
  const msg: Message = {
    id: id(),
    role: 'assistant',
    text: '',
    created_at: now(),
    status: 'streaming',
    provider: target.replace(/^(model|combo):/, ''),
  };
  conversation.messages.push(msg);
  const reply =
    mode === 'plan'
      ? `Here's a plan for **${text.slice(0, 60)}**:\n\n1. Inspect the relevant modules\n2. Make the change behind a small interface\n3. Add tests that prove the behaviour\n4. Review and verify\n\n**Risks:** touching shared state; keep the change small.`
      : `This is the browser preview, so no model was called.\n\nIn the desktop app, Conductor sends your request with only the relevant project context (see the context budget below the prompt) to **${msg.provider}**.\n\n\`\`\`ts\n// example\nconst answer = await conductor.ask(${JSON.stringify(text.slice(0, 40))});\n\`\`\``;
  for (let i = 0; i <= reply.length; i += 12) {
    if (stopped.has(conversation.id)) break;
    msg.text = reply.slice(0, i);
    emit('run-update', {
      conversation_id: conversation.id,
      message_id: msg.id,
      text: msg.text,
      status: 'streaming',
      usage_input: 0,
      usage_output: 0,
      model: msg.provider,
      effort,
      notice: null,
    });
    await delay(/explain everything/i.test(text) ? 150 : 40);
  }
  msg.status = stopped.has(conversation.id) ? 'stopped' : 'complete';
  if (msg.status === 'complete') msg.text = reply;
  stopped.delete(conversation.id);
  emit('run-update', {
    conversation_id: conversation.id,
    message_id: msg.id,
    text: msg.text,
    status: msg.status,
    usage_input: 1200,
    usage_output: 180,
    model: msg.provider,
    effort,
    notice: null,
  });
}
const stopped = new Set<string>();

async function runGoal(rec: GoalRecord) {
  const g = rec.goal;
  const ev = (event: unknown) => emit('engine', { type: 'goal', goal_id: g.id, event });
  const save = () =>
    emit('engine', {
      type: 'goal_saved',
      goal_id: g.id,
      state: g.state,
      done: g.graph.tasks.filter((t) => t.status === 'done').length,
      total: g.graph.tasks.length,
    });
  g.state = 'planning';
  ev({ type: 'state', state: 'planning' });
  await delay(500);
  const mk = (tid: string, title: string, role: string, deps: string[] = []) => ({
    id: tid,
    title,
    detail: '',
    role,
    deps,
    status: 'pending',
    assigned_model: null,
    files: [],
    attempts: 0,
    failed_models: [],
    result: null,
    verification: null,
    started_at: null,
    finished_at: null,
    tokens: 0,
    important: role === 'coder',
  });
  g.graph.tasks = [
    mk('plan', 'Inspect the project and plan', 'planner'),
    mk('impl', 'Implement the change', 'coder', ['plan']),
    mk('review', 'Review the change', 'reviewer', ['impl']),
  ] as GoalRecord['goal']['graph']['tasks'];
  ev({ type: 'planned', tasks: 3 });
  g.state = 'running';
  ev({ type: 'state', state: 'running' });
  save();
  for (const t of g.graph.tasks) {
    if ((g.state as string) === 'stopped') return;
    t.status = 'running';
    t.started_at = now();
    t.assigned_model = profiles()[0] ? `${profiles()[0].provider}/${profiles()[0].model}` : 'preview/model';
    ev({
      type: 'task_started',
      task: t.id,
      title: t.title,
      model: t.assigned_model,
      effort: 'medium',
      reason: 'assigned role',
    });
    save();
    await delay(900);
    emit('engine', {
      type: 'tool',
      goal_id: g.id,
      task_id: t.id,
      tool: 'read_file',
      summary: 'read src/main.rs',
      ok: true,
    });
    await delay(500);
    t.status = 'done';
    t.finished_at = now();
    t.result = 'Done (preview).';
    ev({ type: 'task_finished', task: t.id, status: 'done' });
    save();
  }
  g.state = 'verifying';
  ev({ type: 'state', state: 'verifying' });
  for (const c of g.contract.done_checks) {
    await delay(400);
    const label = c.command ?? c.text ?? '';
    g.checks.push({ check: label, passed: true, evidence: 'ok (preview)' });
    ev({ type: 'check', check: label, passed: true });
  }
  g.state = 'complete';
  ev({ type: 'state', state: 'complete' });
  ev({ type: 'complete' });
  save();
}

const handlers: Record<string, (a: Record<string, unknown>) => unknown> = {
  snapshot: () => snapshot(),
  save_settings: (a) => {
    db.settings = a.settings as Settings;
  },
  app_info: () => ({
    version: '0.1.0',
    data_dir: '(browser preview)',
    portable: false,
    os: 'browser',
    startup: [],
    emergency_shortcut_registered: true,
  }),
  prefs_get: () => db.prefs,
  prefs_save: (a) => {
    db.prefs = a.prefs as Prefs;
  },
  hardware: () => ({
    total_memory_gb: 16,
    logical_cpus: 8,
    recommended: 'balanced',
    reason: '16 GB memory, 8 threads',
  }),
  open_project: (a) => {
    const path = String(a.path);
    const existing = db.projects.find((p) => p.path === path);
    if (existing) return existing;
    const p: Project = {
      id: id(),
      name: path.split(/[\\/]/).filter(Boolean).pop() || path,
      path,
      kind: 'cargo',
      git: true,
      opened_at: now(),
    };
    db.projects.push(p);
    db.settings.last_project = p.id;
    hist(p.id, 'project', 'Project opened');
    return p;
  },
  new_project: (a) => handlers.open_project({ path: `${a.parent}/${a.name}` }),
  project_info: () => ({
    detection: {
      kinds: ['cargo'],
      is_git: true,
      has_submodules: false,
      is_monorepo: false,
      has_conductor_toml: false,
      languages: ['Rust'],
      suggested_test_commands: ['cargo test'],
    },
    config: null,
    config_error: null,
    branch: 'main',
    resume: null,
  }),
  project_remove: (a) => {
    db.projects = db.projects.filter((p) => p.id !== a.projectId);
  },
  recipes: () => [
    { id: 'empty', name: 'Empty folder', description: 'Just a folder with Git.' },
    { id: 'rust-desktop', name: 'Rust app', description: 'Cargo binary crate.' },
    { id: 'threejs-game', name: 'Three.js game', description: 'Vite + Three.js web game.' },
    { id: 'godot-web', name: 'Godot web game', description: 'Godot 4 project skeleton.' },
    { id: 'python', name: 'Python package', description: 'pyproject + pytest.' },
  ],
  save_provider: (a) => {
    const c = a.config as Provider;
    if (c.kind !== 'openai_compatible' && !a.apiKey) throw new Error('API key is required');
    const models =
      c.kind === 'openai'
        ? [
            {
              id: 'gpt-6.1-sol',
              name: 'GPT-6.1 Sol',
              efforts: ['minimal', 'low', 'medium', 'high'],
              context_window: 400000,
              tools: true,
              vision: true,
            },
            {
              id: 'gpt-6-luna',
              name: 'GPT-6 Luna',
              efforts: ['minimal', 'low', 'medium', 'high'],
              context_window: 400000,
              tools: true,
              vision: true,
            },
          ]
        : c.kind === 'anthropic'
          ? [
              {
                id: 'claude-sonnet-5-5',
                name: 'Claude Sonnet 5.5',
                efforts: [],
                context_window: 200000,
                tools: true,
                vision: true,
              },
            ]
          : c.kind === 'gemini'
            ? [
                {
                  id: 'gemini-3.1-pro',
                  name: 'Gemini 3.1 Pro',
                  efforts: [],
                  context_window: 1000000,
                  tools: true,
                  vision: true,
                },
                {
                  id: 'gemini-3.8-flash',
                  name: 'Gemini 3.8 Flash',
                  efforts: [],
                  context_window: 1000000,
                  tools: true,
                  vision: true,
                },
              ]
            : c.base_url.includes('api.x.ai')
              ? [
                  { id: 'grok-4', name: 'Grok 4', efforts: [], context_window: 256000, tools: true, vision: true },
                  {
                    id: 'grok-4-fast',
                    name: 'Grok 4 Fast',
                    efforts: [],
                    context_window: 2000000,
                    tools: true,
                    vision: true,
                  },
                ]
              : [
                  {
                    id: 'local-model',
                    name: 'Local model',
                    efforts: [],
                    context_window: 32000,
                    tools: false,
                    vision: false,
                  },
                ];
    const p = { ...c, models, enabled: true };
    db.providers = db.providers.filter((x) => x.id !== p.id).concat(p);
    hist(null, 'provider', `Connected ${p.name}`);
    return p;
  },
  disconnect_provider: (a) => {
    const p = db.providers.find((x) => x.id === a.id);
    if (p) p.enabled = false;
  },
  usage_overview: () => {
    const win = (limit: number, remaining: number, reset: string) => ({ limit, remaining, reset });
    const none = { limit: null, remaining: null, reset: null };
    const on = !!db.prefs.subscription_signin;
    return {
      providers: db.providers.map((p) => ({
        id: p.id,
        name: p.name,
        kind: p.kind,
        enabled: p.enabled,
        bridge:
          (
            { 'cli-agy': 'gemini', 'cli-codex': 'chatgpt', 'cli-claude': 'claude', 'cli-grok': 'grok' } as Record<
              string,
              string
            >
          )[p.id] ?? null,
        brand:
          (
            { 'cli-agy': 'gemini', 'cli-codex': 'chatgpt', 'cli-claude': 'claude', 'cli-grok': 'grok' } as Record<
              string,
              string
            >
          )[p.id] ??
          (p.base_url.includes('api.x.ai')
            ? 'grok'
            : (({ anthropic: 'claude', openai: 'chatgpt', gemini: 'gemini' } as Record<string, string>)[p.kind] ??
              'other')),
        limits:
          p.kind === 'openai'
            ? {
                requests: win(500, 487, '7.2s'),
                tokens: win(30000, 21400, '17s'),
                input_tokens: none,
                output_tokens: none,
                observed_at: 0,
              }
            : p.kind === 'anthropic'
              ? {
                  requests: win(50, 46, new Date(Date.now() + 40_000).toISOString()),
                  tokens: none,
                  input_tokens: win(40000, 12000, new Date(Date.now() + 40_000).toISOString()),
                  output_tokens: win(8000, 7400, new Date(Date.now() + 40_000).toISOString()),
                  observed_at: 0,
                }
              : null,
        session: { input: 1840, output: 620, requests: 3 },
      })),
      subscriptions_enabled: on,
      subscriptions: (['claude', 'chatgpt', 'gemini'] as const).map((service) => ({
        service,
        label: { claude: 'Claude', chatgpt: 'ChatGPT', gemini: 'Gemini' }[service],
        signed_in: on && service in db.subscriptions,
        account: on ? (db.subscriptions[service]?.account ?? null) : null,
        plan: on ? (db.subscriptions[service]?.plan ?? null) : null,
      })),
    };
  },
  cli_bridges: () => ({
    running: true,
    bridges: (
      [
        ['agy', 'Gemini (Antigravity CLI)', 'gemini', '1.2.16'],
        ['codex', 'ChatGPT (Codex CLI)', 'chatgpt', 'codex-cli 0.153.4'],
        ['claude', 'Claude (Claude Code)', 'claude', '2.1.288 (Claude Code)'],
        ['grok', 'Grok (Grok Build)', 'grok', '0.9.2'],
      ] as const
    ).map(([cli, label, brand, version]) => ({
      cli,
      label,
      brand,
      installed: version !== null,
      version,
      connected: db.providers.some((p) => p.id === `cli-${cli}` && p.enabled),
      signed_in: cli === 'claude' ? false : cli === 'codex' ? true : null,
      sign_in_hint: 'Open a terminal and sign in.',
    })),
  }),
  cli_bridge_login: () => {},
  cli_bridge_connect: async (a) => {
    await new Promise((r) => setTimeout(r, 200));
    const cli = a.cli as string;
    const label = {
      agy: 'Gemini (Antigravity CLI)',
      codex: 'ChatGPT (Codex CLI)',
      claude: 'Claude (Claude Code)',
      grok: 'Grok (Grok Build)',
    }[cli]!;
    const ids = cli === 'agy' ? ['default', 'gemini-3.8-flash-medium', 'gemini-3.1-pro-high'] : ['default'];
    const p: Provider = {
      id: `cli-${cli}`,
      name: label,
      kind: 'openai_compatible',
      base_url: `http://127.0.0.1:47999/${cli}/v1`,
      models: ids.map((id) => ({ id, name: id, efforts: [], context_window: null, tools: false, vision: false })),
      enabled: true,
    };
    db.providers = [...db.providers.filter((x) => x.id !== p.id), p];
    return p;
  },
  subscription_sign_in: async (a) => {
    if (!db.prefs.subscription_signin)
      throw new Error('Subscription sign-in is off. Turn it on in Settings › Usage first.');
    await new Promise((r) => setTimeout(r, 300));
    const plan = { claude: 'Max', chatgpt: 'Plus', gemini: 'Gemini Code Assist for individuals' }[a.service as string];
    db.subscriptions[a.service as string] = { account: 'you@example.com', plan: plan ?? null };
    return db.subscriptions[a.service as string];
  },
  subscription_usage: (a) => {
    const s = a.service as string;
    if (!db.subscriptions[s]) throw new Error('Not signed in.');
    const at = (h: number) => new Date(Date.now() + h * 3_600_000).toISOString();
    const windows =
      s === 'claude'
        ? [
            { label: '5-hour session', used_percent: 42, resets_at: at(2.3), resets_at_unix: null },
            { label: 'Weekly (all models)', used_percent: 18, resets_at: at(90), resets_at_unix: null },
            { label: 'Weekly Opus', used_percent: 86, resets_at: at(90), resets_at_unix: null },
          ]
        : s === 'chatgpt'
          ? [
              {
                label: '5-hour window',
                used_percent: 7,
                resets_at: null,
                resets_at_unix: Math.floor(Date.now() / 1000) + 9000,
              },
              {
                label: 'Weekly window',
                used_percent: 31,
                resets_at: null,
                resets_at_unix: Math.floor(Date.now() / 1000) + 300000,
              },
            ]
          : [
              { label: 'gemini-2.5-flash', used_percent: 3, resets_at: at(11), resets_at_unix: null },
              { label: 'gemini-2.5-pro', used_percent: 25, resets_at: at(11), resets_at_unix: null },
            ];
    return { service: s, account: db.subscriptions[s].account, plan: db.subscriptions[s].plan, windows };
  },
  save_paste: (a) => {
    const text = a.text as string;
    const id = Array.from({ length: 32 }, () => '0123456789abcdef'[Math.floor(Math.random() * 16)]).join('');
    db.pastes[id] = text;
    // Count lines like the backend (str::lines): a trailing newline adds none.
    const lines = Math.max(1, text.split(/\n/).length - (text.endsWith('\n') ? 1 : 0));
    return { id, chars: [...text].length, lines, path: `(preview)/pastes/${id}.txt` };
  },
  profile_info: () => ({ user: 'Alex' }),
  models_refresh: () => 0,
  cli_bridge_update: async () => {
    await new Promise((r) => setTimeout(r, 200));
    return db.providers
      .filter((p) => p.id.startsWith('cli-'))
      .map((p) => ({ app: p.name, ok: true, message: 'Already up to date' }));
  },
  setup_prepare: () => {},
  setup_scan: async () => {
    await new Promise((r) => setTimeout(r, 150));
    return {
      tools: [
        {
          id: 'git',
          name: 'Git',
          state: db.gitInstalled ? 'ok' : 'missing',
          version: db.gitInstalled ? '2.49.0' : null,
          install: ['winget', 'install', '--id', 'Git.Git'],
        },
        { id: 'node', name: 'Node.js', state: 'ok', version: '22.11.0', install: null },
      ],
      bridges: [{ cli: 'agy', label: 'Gemini (Antigravity CLI)', brand: 'gemini', version: '1.2.16' }],
      local: [{ id: 'ollama', label: 'Ollama', url: 'http://localhost:11434/v1', models: 3 }],
    };
  },
  env_install: async (a) => {
    await new Promise((r) => setTimeout(r, 200));
    if (a.tool === 'git') db.gitInstalled = true;
    return 'Git installed.';
  },
  test_notification: () => {
    db.notified += 1;
  },
  subscription_sign_out: (a) => {
    delete db.subscriptions[a.service as string];
  },
  remove_provider: (a) => {
    db.providers = db.providers.filter((x) => x.id !== a.id);
  },
  create_conversation: (a) => {
    const c: Conversation = {
      id: id(),
      project_id: String(a.projectId),
      title: 'New conversation',
      mode: a.mode as Conversation['mode'],
      provider_id: null,
      model_id: null,
      effort: null,
      messages: [],
      updated_at: now(),
    };
    db.conversations.push(c);
    return c;
  },
  rename_conversation: (a) => {
    const c = db.conversations.find((x) => x.id === a.id);
    if (c) c.title = String(a.title);
  },
  delete_conversation: (a) => {
    db.conversations = db.conversations.filter((x) => x.id !== a.id);
  },
  send_message: async (a) => {
    const c = db.conversations.find((x) => x.id === a.conversationId);
    if (!c) throw new Error('Conversation missing');
    if (!profiles().length) throw new Error('Connect a provider first (Settings → Providers).');
    const text = String(a.text);
    if (!text.trim() || new TextEncoder().encode(text).length > 64_000)
      throw new Error('Prompt must contain 1–64,000 bytes');
    if (!c.messages.length) c.title = text.slice(0, 70);
    c.messages.push({ id: id(), role: 'user', text, created_at: now(), status: 'complete', provider: null });
    c.updated_at = now();
    await streamReply(c, text, String(a.target), (a.effort as string) ?? null, String(a.mode));
  },
  stop: (a) => {
    if (a.conversationId) stopped.add(String(a.conversationId));
    else db.conversations.forEach((c) => stopped.add(c.id));
  },
  models: () => profiles(),
  combos_list: () => ({
    combos: [...presets().filter((p) => !db.combos.some((c) => c.id === p.id)), ...db.combos],
    default_combo: db.defaultCombo,
  }),
  combo_save: (a) => {
    const c = { ...(a.combo as Combo), builtin: false };
    db.combos = db.combos.filter((x) => x.id !== c.id).concat(c);
  },
  combo_delete: (a) => {
    db.combos = db.combos.filter((x) => x.id !== a.id);
  },
  combo_duplicate: (a) => {
    const src = [...presets(), ...db.combos].find((c) => c.id === a.id)!;
    const c = { ...structuredClone(src), id: id(), name: String(a.name), builtin: false, version: 1 };
    db.combos.push(c);
    return c;
  },
  combo_set_default: (a) => {
    db.defaultCombo = (a.id as string) ?? null;
  },
  combo_export: (a) =>
    JSON.stringify(
      { format: 'conductor.combo', schema: 1, combo: [...presets(), ...db.combos].find((c) => c.id === a.id) },
      null,
      2,
    ),
  combo_import: (a) => {
    const f = JSON.parse(String(a.json));
    const c = { ...f.combo, id: id(), builtin: false };
    db.combos.push(c);
    return { combo: c, warnings: [] };
  },
  effort_preview: (a) => ({
    level: /debug|race|security|auth/i.test(String(a.prompt))
      ? 'high'
      : /rename|typo/i.test(String(a.prompt))
        ? 'low'
        : 'medium',
    reason: 'heuristic',
  }),
  context_pack: (a): ContextPack => ({
    items: [
      {
        kind: 'rules',
        source: 'project rules',
        reason: 'authoritative project instructions',
        tokens: 42,
        content: 'Never use unwrap in runtime code',
        reduced: false,
        cached: false,
      },
      {
        kind: 'file',
        source: 'src/main.rs',
        reason: `matches "${String(a.prompt).slice(0, 30)}"`,
        tokens: 820,
        content: 'fn main() {\n    println!("hello");\n}',
        reduced: false,
        cached: false,
      },
      {
        kind: 'snippet',
        source: 'src/router.rs',
        reason: 'defines route',
        tokens: 1540,
        content: '  41 pub fn route(...) {',
        reduced: true,
        cached: false,
      },
      {
        kind: 'reference',
        source: 'src/lib.rs',
        reason: 'unchanged since it was last sent',
        tokens: 12,
        content: 'unchanged',
        reduced: true,
        cached: true,
      },
    ],
    omitted: [{ source: 'docs/BIG.md', reason: 'low relevance', tokens: 18000 }],
    stages: [
      { stage: 'repository filtering', detail: '110 files indexed' },
      { stage: 'relevance scoring', detail: '24 candidates' },
      { stage: 'provider prompt construction', detail: '4 items, 1 omitted' },
    ],
    budget_tokens: 16000,
    total_tokens: 2414,
    naive_tokens: 61200,
    cache_reused_tokens: 640,
    duplicates_removed: 0,
    redactions: 0,
    injection_warnings: 0,
    compression_active: true,
    estimated: true,
  }),
  memory_get: () => ({ decisions: [], facts: [], instructions: '' }),
  memory_save: () => undefined,
  goals_list: () =>
    db.goals.map((r) => ({
      id: r.goal.id,
      project_id: r.project_id,
      objective: r.goal.contract.objective,
      state: r.goal.state,
      done: r.goal.graph.tasks.filter((t) => t.status === 'done').length,
      total: r.goal.graph.tasks.length,
      updated: r.updated,
    })),
  goal_get: (a) => db.goals.find((r) => r.goal.id === a.id),
  goal_clarify: (): QuestionRound => ({
    round: 1,
    questions: [
      {
        id: 'q1',
        text: 'Which platforms matter?',
        priority: 'blocking',
        options: ['Windows', 'macOS', 'Linux', 'Web'],
        default: 'Windows',
        topic: 'platforms',
      },
      {
        id: 'q2',
        text: 'Realistic or stylized visuals?',
        priority: 'important',
        options: ['Realistic', 'Stylized'],
        default: 'Stylized',
        topic: 'visual style',
      },
    ],
    dropped: [],
  }),
  goal_create: (a) => {
    const g = a.goal as {
      project_id: string;
      objective: string;
      constraints: string[];
      checks: string[];
      combo_id: string;
    };
    const rec: GoalRecord = {
      goal: {
        id: id(),
        contract: {
          objective: g.objective,
          requirements: [],
          constraints: g.constraints ?? [],
          platforms: [],
          acceptance: [],
          done_checks: (g.checks ?? []).map((c) => ({ kind: 'command', command: c })),
          budget: null,
          permissions: null,
          decisions: [],
          revision: 0,
        },
        graph: { tasks: [] },
        state: 'planning',
        checks: [],
        notes: [],
        verification_rounds: 0,
      },
      project_id: g.project_id,
      project_root: '',
      combo_id: g.combo_id,
      created: now(),
      updated: now(),
      checkpoint: null,
    };
    db.goals.unshift(rec);
    return rec;
  },
  goal_update_contract: (a) => {
    const r = db.goals.find((x) => x.goal.id === a.id)!;
    r.goal.contract = { ...(a.contract as GoalRecord['goal']['contract']), revision: r.goal.contract.revision + 1 };
    return r;
  },
  goal_start: (a) => {
    const r = db.goals.find((x) => x.goal.id === a.id);
    if (!r) throw new Error('Goal not found');
    void runGoal(r);
  },
  goal_stop: (a) => {
    const r = db.goals.find((x) => x.goal.id === a.id);
    if (r) {
      r.goal.state = 'stopped';
      emit('engine', { type: 'goal', goal_id: r.goal.id, event: { type: 'state', state: 'stopped' } });
    }
    return true;
  },
  goal_delete: (a) => {
    db.goals = db.goals.filter((x) => x.goal.id !== a.id);
  },
  approvals_pending: () => [],
  approval_resolve: () => true,
  emergency_stop: () => {
    db.conversations.forEach((c) => stopped.add(c.id));
    db.goals.forEach((g) => (g.goal.state = g.goal.state === 'running' ? 'stopped' : g.goal.state));
    return { chats: 0, goals: 0 };
  },
  mcp_catalog: () => [
    {
      id: 'filesystem',
      name: 'Filesystem',
      description: 'Read and write files in an allowed folder.',
      source: 'https://github.com/modelcontextprotocol/servers',
      command: 'npx',
      args: [],
      needs: ['path'],
      secrets: {},
      dependencies: ['node'],
      keywords: [],
    },
    {
      id: 'github',
      name: 'GitHub',
      description: "Repositories, issues and pull requests via GitHub's official MCP server.",
      source: 'https://github.com/github/github-mcp-server',
      command: 'docker',
      args: [],
      needs: [],
      secrets: { GITHUB_PERSONAL_ACCESS_TOKEN: 'github-token' },
      dependencies: ['docker'],
      keywords: [],
    },
    {
      id: 'blender',
      name: 'Blender',
      description: 'Control Blender (requires the Blender MCP add-on).',
      source: 'https://github.com/ahujasid/blender-mcp',
      command: 'uvx',
      args: [],
      needs: [],
      secrets: {},
      dependencies: ['uv', 'blender'],
      keywords: [],
    },
    {
      id: 'everything',
      name: 'Everything (test server)',
      description: 'Reference server exercising every MCP feature.',
      source: 'https://github.com/modelcontextprotocol/servers',
      command: 'npx',
      args: [],
      needs: [],
      secrets: {},
      dependencies: ['node'],
      keywords: [],
    },
  ],
  mcp_list: () => ({ servers: db.mcp }),
  mcp_add: async (a) => {
    await delay(600);
    db.mcp = db.mcp
      .filter((m) => m.name !== a.id)
      .concat({
        name: String(a.id),
        enabled: true,
        source: `catalog:${a.id}`,
        transport: { type: 'stdio', command: 'npx', args: [], env: {} },
        description: '',
      });
    return {
      server: a.id,
      status: 'connected',
      summary: `${a.id} connected · 13 tool(s)`,
      detail: '',
      tools: ['echo'],
      fix: { kind: 'none' },
    };
  },
  mcp_connect_remote: async (a) => {
    await delay(300);
    db.mcp = db.mcp
      .filter((m) => m.name !== a.name)
      .concat({
        name: String(a.name),
        enabled: true,
        source: String(a.url),
        transport: { type: 'http', url: String(a.url) },
        description: '',
      });
    db.signedConnectors.add(String(a.name));
    return {
      server: a.name,
      status: 'connected',
      summary: `${a.name} connected · 21 tool(s)`,
      detail: '',
      tools: ['search'],
      fix: { kind: 'none' },
    };
  },
  mcp_sign_in: async (a) => {
    await delay(200);
    db.signedConnectors.add(String(a.name));
    return {
      server: a.name,
      status: 'connected',
      summary: `${a.name} connected · 21 tool(s)`,
      detail: '',
      tools: ['search'],
      fix: { kind: 'none' },
    };
  },
  mcp_remove: (a) => {
    db.mcp = db.mcp.filter((m) => m.name !== a.name);
  },
  mcp_set_enabled: (a) => {
    const m = db.mcp.find((x) => x.name === a.name);
    if (m) m.enabled = Boolean(a.enabled);
  },
  mcp_doctor: () =>
    db.mcp.map((m) => ({
      server: m.name,
      status: m.enabled ? 'connected' : 'disabled',
      summary: m.enabled ? `${m.name} connected` : `${m.name} is disabled`,
      detail: '',
      tools: [],
      fix: { kind: m.enabled ? 'none' : 'enable' },
    })),
  mcp_export: () => '{ "mcpServers": {} }',
  secret_set: () => undefined,
  secret_delete: () => undefined,
  skills_list: () => [],
  skills_install: () => {
    throw new Error('Installing skills needs the desktop app.');
  },
  skills_remove: () => undefined,
  plugins_list: () => [],
  plugins_install: () => {
    throw new Error('Installing plugins needs the desktop app.');
  },
  plugins_remove: () => undefined,
  themes_list: () => [],
  themes_install: () => {
    throw new Error('Installing themes needs the desktop app.');
  },
  themes_remove: () => undefined,
  theme_scaffold: () => undefined,
  receipts_list: () => [],
  env_doctor: async () => {
    await delay(400);
    return [
      { id: 'git', name: 'Git', state: 'ok', version: '2.45.0', path: null, detail: '', install: null },
      { id: 'node', name: 'Node.js', state: 'ok', version: '22.0.0', path: null, detail: '', install: null },
      {
        id: 'cmake',
        name: 'CMake',
        state: 'missing',
        version: null,
        path: null,
        detail: "'cmake' not found on PATH",
        install: ['winget', 'install', 'Kitware.CMake'],
      },
    ];
  },
  git_overview: () => ({
    repo: true,
    summary: 'branch main (0 staged, 2 modified, 1 untracked)',
    status: {
      branch: 'main',
      staged: [],
      modified: ['src/main.rs', 'README.md'],
      untracked: ['notes.txt'],
      conflicted: [],
    },
    log: [{ hash: 'a1b2c3d4', author: 'You', date: '2026-10-03T10:00:00Z', subject: 'Initial commit' }],
    branches: ['main'],
    diff: 'diff --git a/README.md b/README.md\n-old\n+new',
  }),
  github_overview: () => ({
    status: 'ok',
    prs: [
      {
        number: 12,
        title: 'Add lobby screen',
        state: 'OPEN',
        url: 'https://github.com/example/repo/pull/12',
        headRefName: 'feature/lobby',
        isDraft: false,
      },
    ],
    issues: [{ number: 7, title: 'Crash on resize', state: 'OPEN', url: 'https://github.com/example/repo/issues/7' }],
    runs: [
      {
        name: 'CI',
        status: 'completed',
        conclusion: 'success',
        url: 'https://github.com/example/repo/actions/runs/1',
        headBranch: 'main',
      },
    ],
  }),
  checkpoints_list: () => [],
  checkpoint_create: (a) => ({ id: id(), commit: 'abc', label: String(a.label), created: new Date().toISOString() }),
  checkpoint_restore: () => ({ id: id(), commit: 'def', label: 'before restore', created: new Date().toISOString() }),
  remote_status: () => ({
    running: db.host,
    addr: db.host ? '127.0.0.1:47820' : null,
    fingerprint: db.host
      ? 'AB:CD:EF:12:34:56:78:90:AB:CD:EF:12:34:56:78:90:AB:CD:EF:12:34:56:78:90:AB:CD:EF:12:34:56:78:90'
      : null,
    devices: [],
    connected: 0,
  }),
  remote_start: () => {
    db.host = true;
    return handlers.remote_status({});
  },
  remote_stop: () => {
    db.host = false;
  },
  remote_pair_code: () => ({
    code: 'K7QM-4TZP',
    expires_at: now() + 300,
    fingerprint: 'AB:CD:EF:…',
    addr: '127.0.0.1:47820',
  }),
  remote_revoke: () => true,
  caveman_status: () => ({
    enabled: db.settings.caveman,
    active: 'builtin-1',
    pending: null,
    previous: null,
    last_error: null,
    upstream: 'https://github.com/JuliusBrussee/caveman',
    license: 'Apache-2.0',
  }),
  caveman_update: () => {
    throw new Error('Updating Caveman needs the desktop app.');
  },
  caveman_rollback: () => undefined,
  ports: () => [{ port: 1420, address: '127.0.0.1', pid: 4242, dev: true }],
  tunnels_list: () => ({ available: false, tunnels: [] }),
  tunnel_open: () => {
    throw new Error('Tunnels need the desktop app.');
  },
  tunnel_close: () => true,
  config_export: () =>
    JSON.stringify({ format: 'conductor.config', schema: 1, note: 'No secrets are included.' }, null, 2),
  config_import: () => ['Settings imported'],
  diagnostics_export: () => '(browser preview)',
  reveal_path: () => undefined,
  quit_app: () => undefined,
  hide_window: () => undefined,
};

export const mockBridge = {
  async call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
    const h = handlers[cmd];
    if (!h) throw new Error(`Not available in the browser preview: ${cmd}`);
    await delay(20);
    // Serialize like real IPC so UI state proxies never leak into the mock store.
    const plain = JSON.parse(JSON.stringify(args)) as Record<string, unknown>;
    const result = await h(plain);
    return (result === undefined ? undefined : JSON.parse(JSON.stringify(result))) as T;
  },
  async listen<T>(event: string, cb: (p: T) => void) {
    const set = listeners.get(event) ?? new Set();
    set.add(cb as Listener);
    listeners.set(event, set);
    return () => set.delete(cb as Listener);
  },
  async chooseFolder(): Promise<string | null> {
    const p = window.prompt('Folder path (browser preview)', 'C:\\Projects\\demo-app');
    return p && p.trim() ? p.trim() : null;
  },
};

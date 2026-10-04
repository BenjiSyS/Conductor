// Types mirror the Rust structs serialized over Tauri IPC.

export type Mode = 'chat' | 'plan' | 'goal' | 'agent';
export type Permission = 'ask' | 'auto_approve' | 'full_access';
export type Performance = 'potato' | 'balanced' | 'maximum';

export interface Settings {
  setup_complete: boolean;
  theme: string;
  performance: Performance;
  permission: Permission;
  auto_approve: string[];
  instructions: string;
  compression: boolean;
  caveman: boolean;
  allow_highest_effort: boolean;
  last_project: string | null;
}

export interface Prefs {
  close_behavior: 'background' | 'exit' | 'ask';
  theme_mode: 'system' | 'light' | 'dark';
  theme_name: string;
  reduced_motion: boolean;
  ui_scale: number;
  privacy: 'local_first' | 'standard' | 'restricted';
  telemetry: boolean;
  update_channel: 'stable' | 'beta' | 'nightly';
  auto_update_check: boolean;
  launch_at_login: boolean;
  routing: string;
  max_effort: Record<string, 'ask' | 'always_allow' | 'never'>;
  provider_instructions: Record<string, string>;
  role_instructions: Record<string, string>;
  keybindings: Record<string, string>;
  favorites: string[];
  recents: string[];
  default_target: string | null;
  remote_enabled: boolean;
  remote_bind: string;
  remote_port: number;
  remote_continue_on_disconnect: boolean;
  emergency_shortcut: string;
  subscription_signin: boolean;
  seen_resume: Record<string, number>;
}

export type ProviderKind = 'openai' | 'anthropic' | 'gemini' | 'openai_compatible';
export interface Model {
  id: string;
  name: string;
  efforts: string[];
  context_window: number | null;
  tools: boolean;
  vision: boolean;
}
export interface Provider {
  id: string;
  name: string;
  kind: ProviderKind;
  base_url: string;
  enabled: boolean;
  models: Model[];
}
export interface Project {
  id: string;
  name: string;
  path: string;
  kind: string;
  git: boolean;
  opened_at: number;
}
export interface Message {
  id: string;
  role: 'user' | 'assistant' | 'system';
  text: string;
  created_at: number;
  status: string;
  provider: string | null;
}
export interface Conversation {
  id: string;
  project_id: string;
  title: string;
  mode: Mode;
  provider_id: string | null;
  model_id: string | null;
  effort: string | null;
  messages: Message[];
  updated_at: number;
}
export interface HistoryEntry {
  id: string;
  project_id: string | null;
  kind: string;
  summary: string;
  created_at: number;
}
export interface Snapshot {
  settings: Settings;
  projects: Project[];
  providers: Provider[];
  conversations: Conversation[];
  history: HistoryEntry[];
  data_dir: string;
}

export type Tier = 'local' | 'fast' | 'strong' | 'frontier';
export interface ModelProfile {
  provider: string;
  model: string;
  display: string;
  tier: Tier;
  capabilities: string[];
  efforts: string[];
  context_window: number;
  local: boolean;
}

export type AgentRole =
  | 'architect'
  | 'planner'
  | 'coder'
  | 'reviewer'
  | 'researcher'
  | 'debugger'
  | 'tester'
  | 'release_manager'
  | 'documentation'
  | 'security_reviewer';

export interface ComboMember {
  model: string;
  roles: AgentRole[];
  effort: string | null;
  enabled: boolean;
  weight: number;
}
export interface Reserve {
  provider: string;
  fraction: number;
  for_roles: AgentRole[];
}
export interface Combo {
  id: string;
  name: string;
  description: string;
  version: number;
  members: ComboMember[];
  strategy: string;
  strategy_provider: string | null;
  reserves: Reserve[];
  fallback: boolean;
  review: 'off' | 'important' | 'always';
  max_parallel: number;
  budget: { max_tokens: number | null; max_cost_usd: number | null; ask_at_fraction: number | null };
  instructions: string;
  phases: unknown[];
  builtin: boolean;
  history: { version: number; note: string }[];
}

export interface RunUpdate {
  conversation_id: string;
  message_id: string;
  text: string;
  status: string;
  usage_input: number;
  usage_output: number;
  model: string;
  effort: string | null;
  notice: string | null;
}

export interface ContextItem {
  kind: string;
  source: string;
  reason: string;
  tokens: number;
  content: string;
  reduced: boolean;
  cached: boolean;
}
export interface ContextPack {
  items: ContextItem[];
  omitted: { source: string; reason: string; tokens: number }[];
  stages: { stage: string; detail: string }[];
  budget_tokens: number;
  total_tokens: number;
  naive_tokens: number;
  cache_reused_tokens: number;
  duplicates_removed: number;
  redactions: number;
  injection_warnings: number;
  compression_active: boolean;
  estimated: boolean;
}

export type GoalState =
  | 'clarifying'
  | 'planning'
  | 'running'
  | 'verifying'
  | 'waiting_for_user'
  | 'paused'
  | 'blocked'
  | 'complete'
  | 'stopped';
export type TaskStatus = 'pending' | 'ready' | 'running' | 'review' | 'done' | 'failed' | 'blocked' | 'cancelled';
export interface DoneCheck {
  kind: 'command' | 'criterion';
  command?: string;
  text?: string;
}
export interface GoalContract {
  objective: string;
  requirements: string[];
  constraints: string[];
  platforms: string[];
  acceptance: string[];
  done_checks: DoneCheck[];
  budget: string | null;
  permissions: string | null;
  decisions: string[];
  revision: number;
}
export interface TaskNode {
  id: string;
  title: string;
  detail: string;
  role: AgentRole;
  deps: string[];
  status: TaskStatus;
  assigned_model: string | null;
  files: string[];
  attempts: number;
  failed_models: string[];
  result: string | null;
  verification: string | null;
  started_at: number | null;
  finished_at: number | null;
  tokens: number;
  important: boolean;
}
export interface Goal {
  id: string;
  contract: GoalContract;
  graph: { tasks: TaskNode[] };
  state: GoalState;
  checks: { check: string; passed: boolean; evidence: string }[];
  notes: string[];
  verification_rounds: number;
}
export interface GoalRecord {
  goal: Goal;
  project_id: string;
  project_root: string;
  combo_id: string;
  created: number;
  updated: number;
  checkpoint: string | null;
}
export interface GoalSummary {
  id: string;
  project_id: string;
  objective: string;
  state: GoalState;
  done: number;
  total: number;
  updated: number;
}

export interface Question {
  id: string;
  text: string;
  priority: 'blocking' | 'important' | 'optional';
  options: string[];
  default: string | null;
  topic: string;
}
export interface QuestionRound {
  round: number;
  questions: Question[];
  dropped: [string, string][];
}
export type Answer = { kind: 'text'; value: string } | { kind: 'option'; value: string } | { kind: 'decide_for_me' };

export interface ApprovalRequest {
  id: string;
  goal_id: string | null;
  capability: string;
  detail: string;
  risk: string;
}

export type RunnerEvent =
  | { type: 'state'; state: GoalState }
  | { type: 'planned'; tasks: number }
  | { type: 'task_started'; task: string; title: string; model: string; effort: string | null; reason: string }
  | { type: 'task_finished'; task: string; status: TaskStatus }
  | { type: 'reused'; task: string; from_model: string }
  | { type: 'handoff'; task: string; from: string; to: string; reason: string; packet_tokens: number }
  | { type: 'notice'; message: string }
  | { type: 'effort_request'; model: string; level: string; why: string }
  | { type: 'check'; check: string; passed: boolean }
  | { type: 'blocked'; reason: string }
  | { type: 'complete' };

export type EngineEvent =
  | { type: 'goal'; goal_id: string; event: RunnerEvent }
  | { type: 'delta'; goal_id: string | null; task_id: string; text: string }
  | { type: 'tool'; goal_id: string | null; task_id: string; tool: string; summary: string; ok: boolean }
  | ({ type: 'approval' } & ApprovalRequest)
  | { type: 'approval_resolved'; id: string; allowed: boolean }
  | { type: 'goal_saved'; goal_id: string; state: string; done: number; total: number };

export interface Diagnosis {
  server: string;
  status: string;
  summary: string;
  detail: string;
  tools: string[];
  fix: { kind: string; tool?: string; hint?: string; name?: string };
}
export interface CatalogEntry {
  id: string;
  name: string;
  description: string;
  source: string;
  command: string;
  args: string[];
  needs: string[];
  secrets: Record<string, string>;
  dependencies: string[];
  keywords: string[];
}
export interface McpServer {
  name: string;
  transport:
    { type: 'stdio'; command: string; args: string[]; env: Record<string, unknown> } | { type: 'http'; url: string };
  enabled: boolean;
  description: string;
  source: string;
}
export interface ToolReport {
  id: string;
  name: string;
  state: 'ok' | 'missing' | 'broken' | 'too_old';
  version: string | null;
  path: string | null;
  detail: string;
  install: string[] | null;
}
export interface SkillInfo {
  manifest: { name: string; version: string; description: string; permissions: string[]; mcp: string[] };
  path: string;
  sensitive: string[];
}
export interface ThemeManifest {
  name: string;
  version: string;
  author: string;
  description: string;
  light: Record<string, string>;
  dark: Record<string, string>;
}
export interface HostStatus {
  running: boolean;
  addr: string | null;
  fingerprint: string | null;
  devices: {
    id: string;
    name: string;
    projects: string[];
    created: number;
    last_seen: number | null;
    can_control: boolean;
  }[];
  connected: number;
}
export interface Checkpoint {
  id: string;
  commit: string;
  label: string;
  created: string;
}
export interface Recipe {
  id: string;
  name: string;
  description: string;
}
export interface ProjectMemory {
  decisions: { id: string; topic: string; value: string; at: number; source: string }[];
  facts: string[];
  instructions: string;
}

export const defaultSettings: Settings = {
  setup_complete: false,
  theme: 'system',
  performance: 'balanced',
  permission: 'ask',
  auto_approve: [],
  instructions: '',
  compression: true,
  caveman: true,
  allow_highest_effort: false,
  last_project: null,
};

export interface RateWindow {
  limit: number | null;
  remaining: number | null;
  reset: string | null;
}
export interface UsageOverview {
  providers: {
    id: string;
    name: string;
    kind: ProviderKind;
    enabled: boolean;
    limits: {
      requests: RateWindow;
      tokens: RateWindow;
      input_tokens: RateWindow;
      output_tokens: RateWindow;
      observed_at: number;
    } | null;
    session: { input: number; output: number; requests: number };
  }[];
  subscriptions_enabled: boolean;
  subscriptions: {
    service: 'claude' | 'chatgpt' | 'gemini';
    label: string;
    signed_in: boolean;
    account: string | null;
    plan: string | null;
  }[];
}
export interface SubscriptionUsage {
  service: 'claude' | 'chatgpt' | 'gemini';
  account: string | null;
  plan: string | null;
  windows: { label: string; used_percent: number; resets_at: string | null; resets_at_unix: number | null }[];
}

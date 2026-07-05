import { InitInput, InitOutput } from './pkg/mellowmesh_wasm.js';

export interface BrowserMellowMeshConfig {
  mode?: 'standalone' | 'client';
  daemonUrl?: string;
  daemonHttpUrl?: string;
  broadcastChannelName?: string | null;
  persistenceKey?: string | null;
  maxHistory?: number;
}

export interface MellowMeshMessage {
  id?: string;
  topic: string;
  from: string;
  owner?: string | null;
  timestamp?: string;
  content_type?: string;
  body?: string;
  headers?: Record<string, string> | null;
  payload?: any;
}

export interface AgentRegistration {
  id: string;
  name: string;
  capabilities: string[];
  [key: string]: any;
}

export interface Task {
  id?: string;
  title: string;
  description?: string | null;
  created_from?: string | null;
  created_by: string;
  status?: string;
  priority?: 'low' | 'medium' | 'high';
  topics?: string[];
  required_capabilities?: string[];
  assigned_to?: string | null;
  claimed_by?: string | null;
  deadline?: string | null;
  artifacts?: any[];
  decisions?: any[];
  lease_seconds?: number | null;
  claim_expires_at?: string | null;
  [key: string]: any;
}

export interface DecisionOption {
  id: string;
  label: string;
  pros?: string[];
  cons?: string[];
}

export interface Decision {
  responded_by?: string | null;
  id?: string;
  title: string;
  question: string;
  created_by: string;
  required_decider: string;
  status?: string;
  options: DecisionOption[];
  response_option_id?: string | null;
  response_timestamp?: string | null;
}

export class BrowserMellowMesh {
  mode: 'standalone' | 'client';
  daemonUrl: string;
  daemonHttpUrl: string;
  broadcastChannelName: string | null;
  persistenceKey: string | null;
  maxHistory: number;
  initialized: boolean;

  constructor(config?: BrowserMellowMeshConfig);
  init(wasmModule?: InitInput): Promise<InitOutput>;
  publish(message: MellowMeshMessage): Promise<string>;
  subscribe(pattern: string, callback: (message: MellowMeshMessage) => void): string;
  unsubscribe(subId: string): boolean;
  registerAgent(agent: AgentRegistration): Promise<void>;
  listAgents(): Promise<AgentRegistration[]>;
  createTask(task: Task): Promise<void>;
  listTasks(): Promise<Task[]>;
  claimTask(taskId: string, agentUri: string): Promise<boolean>;
  completeTask(taskId: string): Promise<boolean>;
  createDecision(decision: Decision): Promise<void>;
  listDecisions(): Promise<Decision[]>;
  respondDecision(decisionId: string, optionId: string): Promise<boolean>;
  readHistory(pattern: string, limit?: number): Promise<MellowMeshMessage[]>;
  clearState(): void;
  close(): void;
}

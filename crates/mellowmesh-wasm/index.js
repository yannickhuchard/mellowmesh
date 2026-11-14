import init, { WasmMellowMeshNode, init_panic_hook } from './pkg/mellowmesh_wasm.js';

/**
 * Encodes a single identifier (task id, decision id, ...) for use as one URL
 * path segment. Rejects empty ids and the dot segments `.` / `..`, which would
 * otherwise be normalized away and address a different endpoint.
 * @param {string} id
 * @returns {string}
 */
export function encodeIdSegment(id) {
  const value = String(id ?? '');
  if (value === '' || value === '.' || value === '..') {
    throw new Error(`Invalid identifier: ${JSON.stringify(value)}`);
  }
  return encodeURIComponent(value);
}

/**
 * Encodes a slash-separated path (e.g. a wiki page path) segment by segment,
 * keeping `/` as the separator. Empty, `.` and `..` segments are rejected.
 * @param {string} path
 * @returns {string}
 */
export function encodePathSegments(path) {
  return String(path ?? '')
    .split('/')
    .map((segment) => encodeIdSegment(segment))
    .join('/');
}

/**
 * Topic pattern matcher with the same semantics as the Rust core
 * (`mellowmesh_core::topic::match_topic_with_options`): `*` matches exactly one
 * segment, `**` matches zero or more segments, and a trailing `>` matches one
 * or more segments. Runs in O(pattern × topic) time and memory.
 * @param {string} pattern
 * @param {string} topic
 * @param {boolean} [caseInsensitive=false]
 * @returns {boolean}
 */
export function matchTopicPattern(pattern, topic, caseInsensitive = false) {
  const p = String(pattern).split('.');
  const t = String(topic).split('.');
  const norm = caseInsensitive ? (s) => s.toLowerCase() : (s) => s;
  const pn = p.length;
  const tn = t.length;

  // After processing pattern index i: next[j] === true <=> p[i..] matches t[j..].
  let next = new Array(tn + 1).fill(false);
  next[tn] = true; // empty pattern matches only the empty remainder
  for (let i = pn - 1; i >= 0; i--) {
    const seg = p[i];
    const cur = new Array(tn + 1).fill(false);
    if (seg === '**') {
      if (i === pn - 1) {
        cur.fill(true);
      } else {
        // ** consumes zero or more segments: cur[j] = OR_{k>=j} next[k]
        let any = false;
        for (let j = tn; j >= 0; j--) {
          any = any || next[j];
          cur[j] = any;
        }
      }
    } else if (seg === '>') {
      // Only valid as the final segment, where it needs at least one segment.
      if (i === pn - 1) {
        for (let j = 0; j < tn; j++) cur[j] = true;
      }
    } else {
      const needle = norm(seg);
      for (let j = 0; j < tn; j++) {
        cur[j] = next[j + 1] && (seg === '*' || needle === norm(t[j]));
      }
    }
    next = cur;
  }
  return next[0];
}

export class BrowserMellowMesh {
  /**
   * Creates a BrowserMellowMesh manager.
   * @param {Object} config
   * @param {string} [config.mode="standalone"] - "standalone" (in-browser node) or "client" (connect to mellowmeshd daemon)
   * @param {string} [config.daemonUrl="ws://127.0.0.1:40000/ws"] - Address of daemon (used in client mode)
   * @param {string} [config.daemonHttpUrl="http://127.0.0.1:40000"] - Http address of daemon
   * @param {string} [config.token] - Bearer token for the daemon (client mode). Sent as
   *   `Authorization: Bearer` on HTTP requests and as `?token=` on the WebSocket URL.
   * @param {string} [config.broadcastChannelName="mellowmesh_broadcast"] - BroadcastChannel name for cross-tab sync
   * @param {string} [config.persistenceKey="mellowmesh_state"] - LocalStorage key to persist state (standalone only)
   * @param {number} [config.maxHistory=500] - Max messages to retain in standalone memory (0 = unlimited)
   * @param {number} [config.saveDebounceMs=250] - Delay used to coalesce LocalStorage writes
   */
  constructor(config = {}) {
    this.mode = config.mode || 'standalone';
    this.daemonUrl = config.daemonUrl || 'ws://127.0.0.1:40000/ws';
    this.daemonHttpUrl = config.daemonHttpUrl || 'http://127.0.0.1:40000';
    this.token = config.token || null;
    this.broadcastChannelName = config.broadcastChannelName === undefined
      ? 'mellowmesh_broadcast'
      : config.broadcastChannelName;
    this.persistenceKey = config.persistenceKey === undefined
      ? 'mellowmesh_state'
      : config.persistenceKey;
    this.maxHistory = Number.isInteger(config.maxHistory) && config.maxHistory >= 0
      ? config.maxHistory
      : 500;
    this.saveDebounceMs = Number.isFinite(config.saveDebounceMs) && config.saveDebounceMs >= 0
      ? config.saveDebounceMs
      : 250;

    this.node = null;
    this.bc = null;
    this.socket = null;
    this.clientSubscriptions = new Map(); // id -> { pattern, callback, caseInsensitive } (client mode)
    this.jsSubscriptions = new Map(); // id -> { pattern, callback } (standalone mode; id is the Rust subscription id)
    this.initialized = false;
    this.reconnectTimer = null;
    this.saveTimer = null;
    this.destroyed = false;
  }

  /**
   * Initializes the WASM engine, local storage, and messaging bridges.
   * @param {WebAssembly.Module|BufferSource|string} [wasmModule] - Optional custom WASM module, buffer, or path
   */
  async init(wasmModule = undefined) {
    if (this.initialized) return;

    // 1. Initialize WASM
    if (wasmModule !== undefined) {
      await init({ module_or_path: wasmModule });
    } else {
      await init();
    }
    init_panic_hook();

    if (this.mode === 'standalone') {
      // 2. Initialize in-browser local coordinator node
      this.node = new WasmMellowMeshNode();
      this.node.set_max_history(this.maxHistory);

      // 3. Load persisted state if configured
      if (this.persistenceKey) {
        try {
          const saved = localStorage.getItem(this.persistenceKey);
          if (saved) {
            this.node.load_state(JSON.parse(saved));
          }
        } catch (e) {
          console.warn('Failed to restore MellowMesh state from localStorage:', e);
        }
      }

      // 4. Set up cross-tab BroadcastChannel bridging
      if (this.broadcastChannelName) {
        this.bc = new BroadcastChannel(this.broadcastChannelName);
        this.bc.onmessage = (event) => {
          const { type, data } = event.data || {};
          // Receive actions from other tabs and run them locally WITHOUT re-broadcasting
          try {
            if (type === 'publish') {
              this.node.publish(data);
            } else if (type === 'register_agent') {
              this.node.register_agent(data);
            } else if (type === 'create_task') {
              this.node.create_task(data);
            } else if (type === 'claim_task') {
              this.node.claim_task(data.taskId, data.agentUri);
            } else if (type === 'complete_task') {
              this.node.complete_task(data.taskId);
            } else if (type === 'create_decision') {
              this.node.create_decision(data);
            } else if (type === 'respond_decision') {
              this.node.respond_decision(data.decisionId, data.optionId, data.responder ?? undefined);
            } else {
              return;
            }
          } catch (e) {
            console.warn(`Failed to apply cross-tab '${type}' action:`, e);
            return;
          }
          this.scheduleSave();
        };
      }
    } else {
      // 5. Initialize Client mode (connects to running mellowmeshd background process)
      await this.connectToDaemon();
    }

    this.initialized = true;
  }

  /**
   * Schedules a debounced write of the standalone state to LocalStorage, so a
   * burst of operations serializes the state only once.
   */
  scheduleSave() {
    if (this.mode !== 'standalone' || !this.persistenceKey || !this.node) return;
    if (this.saveTimer) return;
    this.saveTimer = setTimeout(() => {
      this.saveTimer = null;
      this.saveStateLocally();
    }, this.saveDebounceMs);
  }

  /**
   * Writes the standalone state to LocalStorage immediately, cancelling any
   * pending debounced write. Storage failures (quota, disabled storage) are
   * logged and never thrown.
   */
  saveStateLocally() {
    if (this.saveTimer) {
      clearTimeout(this.saveTimer);
      this.saveTimer = null;
    }
    if (this.mode === 'standalone' && this.persistenceKey && this.node) {
      try {
        const state = this.node.get_state();
        localStorage.setItem(this.persistenceKey, JSON.stringify(state));
      } catch (e) {
        console.error('Failed to save MellowMesh state to localStorage:', e);
      }
    }
  }

  /**
   * Builds request headers, adding the bearer token when configured.
   */
  authHeaders(extra = {}) {
    const headers = { ...extra };
    if (this.token) {
      headers['Authorization'] = `Bearer ${this.token}`;
    }
    return headers;
  }

  /**
   * Performs an HTTP request against the daemon. `path` must already be
   * encoded (see encodeIdSegment / encodePathSegments).
   */
  daemonFetch(path, options = {}) {
    return fetch(`${this.daemonHttpUrl}${path}`, {
      ...options,
      headers: this.authHeaders(options.headers || {})
    });
  }

  async daemonJson(path) {
    const response = await this.daemonFetch(path);
    if (!response.ok) {
      throw new Error(`Daemon request ${path} failed: ${response.statusText}`);
    }
    return response.json();
  }

  /**
   * WebSocket URL including the token query parameter when configured.
   */
  webSocketUrl() {
    if (!this.token) return this.daemonUrl;
    const url = new URL(this.daemonUrl);
    url.searchParams.set('token', this.token);
    return url.toString();
  }

  /**
   * Establishes a WebSocket connection to the native local mellowmeshd daemon.
   */
  connectToDaemon() {
    if (this.destroyed) return Promise.reject(new Error('Instance is destroyed'));

    return new Promise((resolve, reject) => {
      this.socket = new WebSocket(this.webSocketUrl());

      this.socket.onopen = () => {
        if (this.destroyed) {
          this.socket.close();
          return;
        }
        console.log('Connected to MellowMesh native daemon:', this.daemonUrl);
        resolve();
      };

      this.socket.onerror = (err) => {
        // Only log errors if we are active
        if (!this.destroyed) {
          console.error('MellowMesh daemon connection failed:', err);
        }
        reject(err);
      };

      this.socket.onmessage = (event) => {
        if (this.destroyed) return;
        try {
          const message = JSON.parse(event.data);
          // Check matching subscriptions
          for (const sub of this.clientSubscriptions.values()) {
            if (matchTopicPattern(sub.pattern, message.topic, sub.caseInsensitive)) {
              sub.callback(message);
            }
          }
        } catch (e) {
          console.warn('Failed to parse WebSocket message from daemon:', e);
        }
      };

      this.socket.onclose = () => {
        if (this.destroyed) return;
        console.log('MellowMesh daemon connection closed. Attempting reconnect in 3s...');
        this.reconnectTimer = setTimeout(() => {
          this.connectToDaemon().catch(() => {});
        }, 3000);
      };
    });
  }

  /**
   * Helper pattern matcher for JS client-side filtering.
   */
  matchTopicPattern(pattern, topic, caseInsensitive = false) {
    return matchTopicPattern(pattern, topic, caseInsensitive);
  }

  // ==========================================
  // CORE API METHODS
  // ==========================================

  /**
   * Publishes a message onto the coordination fabric.
   */
  async publish(message) {
    // Fill in default fields if missing
    const msg = {
      id: message.id || '',
      topic: message.topic,
      from: message.from,
      owner: message.owner || null,
      timestamp: message.timestamp || new Date().toISOString(),
      content_type: message.content_type || 'text/plain',
      body: message.body || '',
      headers: message.headers || null,
      payload: message.payload || null,
    };

    if (this.mode === 'standalone') {
      const msgId = this.node.publish(msg);
      this.scheduleSave();

      // Broadcast to other tabs
      if (this.bc) {
        msg.id = msgId;
        this.bc.postMessage({ type: 'publish', data: msg });
      }
      return msgId;
    } else {
      // POST to daemon
      const response = await this.daemonFetch('/publish', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(msg)
      });
      if (!response.ok) {
        throw new Error(`Failed to publish message to daemon: ${response.statusText}`);
      }
      const text = await response.text();
      return text;
    }
  }

  /**
   * Subscribes to a topic wildcard pattern.
   * @param {string} pattern - Dot-separated topic pattern (e.g. "_task.**")
   * @param {function} callback - Callback function(message)
   * @param {Object} [options]
   * @param {boolean} [options.caseInsensitive=false] - Match topic segments case-insensitively
   * @returns {string} Subscription ID
   */
  subscribe(pattern, callback, options = {}) {
    const caseInsensitive = Boolean(options.caseInsensitive);
    if (this.mode === 'standalone') {
      // The Rust engine matches case-sensitively; for case-insensitive
      // subscriptions it delivers everything and the JS matcher filters.
      const handler = caseInsensitive
        ? (msg) => {
          if (matchTopicPattern(pattern, msg.topic, true)) callback(msg);
        }
        : callback;
      const subId = this.node.subscribe(caseInsensitive ? '**' : pattern, handler);
      this.jsSubscriptions.set(subId, { pattern, callback });
      return subId;
    }
    const subId = `sub_${Math.random().toString(36).substring(2, 11)}`;
    this.clientSubscriptions.set(subId, { pattern, callback, caseInsensitive });
    return subId;
  }

  /**
   * Cancels an active subscription.
   */
  unsubscribe(subId) {
    if (this.mode === 'standalone') {
      const removed = this.node ? this.node.unsubscribe(subId) : false;
      return this.jsSubscriptions.delete(subId) || removed;
    } else {
      return this.clientSubscriptions.delete(subId);
    }
  }

  /**
   * Registers an agent's capability profile.
   */
  async registerAgent(agent) {
    if (this.mode === 'standalone') {
      this.node.register_agent(agent);
      this.scheduleSave();
      if (this.bc) {
        this.bc.postMessage({ type: 'register_agent', data: agent });
      }
    } else {
      const response = await this.daemonFetch('/agents', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(agent)
      });
      if (!response.ok) {
        throw new Error(`Failed to register agent: ${response.statusText}`);
      }
    }
  }

  /**
   * Retrieves all registered agent profiles.
   */
  async listAgents() {
    if (this.mode === 'standalone') {
      return this.node.list_agents();
    } else {
      return this.daemonJson('/agents');
    }
  }

  /**
   * Creates a new task.
   */
  async createTask(task) {
    const defaultTask = {
      id: task.id || '',
      title: task.title,
      description: task.description || null,
      created_from: task.created_from || null,
      created_by: task.created_by,
      status: task.status || 'open',
      priority: task.priority || 'medium',
      topics: task.topics || [],
      required_capabilities: task.required_capabilities || [],
      assigned_to: task.assigned_to || null,
      claimed_by: task.claimed_by || null,
      deadline: task.deadline || null,
      artifacts: task.artifacts || [],
      decisions: task.decisions || [],
      lease_seconds: task.lease_seconds ?? null,
      claim_expires_at: task.claim_expires_at || null,
    };

    if (this.mode === 'standalone') {
      this.node.create_task(defaultTask);
      this.scheduleSave();
      if (this.bc) {
        this.bc.postMessage({ type: 'create_task', data: defaultTask });
      }
    } else {
      const response = await this.daemonFetch('/tasks', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(defaultTask)
      });
      if (!response.ok) {
        throw new Error(`Failed to create task: ${response.statusText}`);
      }
    }
  }

  /**
   * Lists all current tasks.
   */
  async listTasks() {
    if (this.mode === 'standalone') {
      return this.node.list_tasks();
    } else {
      return this.daemonJson('/tasks');
    }
  }

  /**
   * Claims an open task for an agent. Fails (returns false) when the task is
   * claimed by another agent (unexpired lease) or already closed.
   */
  async claimTask(taskId, agentUri) {
    if (this.mode === 'standalone') {
      const success = this.node.claim_task(taskId, agentUri);
      if (success) {
        this.scheduleSave();
        if (this.bc) {
          this.bc.postMessage({ type: 'claim_task', data: { taskId, agentUri } });
        }
      }
      return success;
    } else {
      const response = await this.daemonFetch(`/tasks/${encodeIdSegment(taskId)}/claim`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ claimed_by: agentUri })
      });
      return response.ok;
    }
  }

  /**
   * Marks a claimed task as completed.
   */
  async completeTask(taskId) {
    if (this.mode === 'standalone') {
      const success = this.node.complete_task(taskId);
      if (success) {
        this.scheduleSave();
        if (this.bc) {
          this.bc.postMessage({ type: 'complete_task', data: { taskId } });
        }
      }
      return success;
    } else {
      const response = await this.daemonFetch(`/tasks/${encodeIdSegment(taskId)}/complete`, {
        method: 'POST'
      });
      return response.ok;
    }
  }

  /**
   * Creates a new decision approval request.
   */
  async createDecision(decision) {
    const defaultDecision = {
      id: decision.id || '',
      title: decision.title,
      question: decision.question,
      created_by: decision.created_by,
      required_decider: decision.required_decider,
      status: decision.status || 'requested',
      options: decision.options || [],
      response_option_id: decision.response_option_id || null,
      response_timestamp: decision.response_timestamp || null,
    };

    if (this.mode === 'standalone') {
      this.node.create_decision(defaultDecision);
      this.scheduleSave();
      if (this.bc) {
        this.bc.postMessage({ type: 'create_decision', data: defaultDecision });
      }
    } else {
      const response = await this.daemonFetch('/decisions', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(defaultDecision)
      });
      if (!response.ok) {
        throw new Error(`Failed to create decision request: ${response.statusText}`);
      }
    }
  }

  /**
   * Lists all current decision requests.
   */
  async listDecisions() {
    if (this.mode === 'standalone') {
      return this.node.list_decisions();
    } else {
      return this.daemonJson('/decisions');
    }
  }

  /**
   * Submits a decision response from a decider.
   * @param {string} decisionId
   * @param {string} optionId - Must be one of the decision's option ids
   * @param {string} [responder] - Who answers; defaults to the decision's required_decider
   *   in standalone mode. The decision's creator cannot answer its own decision, and an
   *   already answered decision cannot be answered again.
   * @returns {Promise<boolean>} Whether the response was recorded
   */
  async respondDecision(decisionId, optionId, responder = undefined) {
    if (this.mode === 'standalone') {
      const success = this.node.respond_decision(decisionId, optionId, responder ?? undefined);
      if (success) {
        this.scheduleSave();
        if (this.bc) {
          this.bc.postMessage({
            type: 'respond_decision',
            data: { decisionId, optionId, responder: responder ?? null }
          });
        }
      }
      return success;
    } else {
      const body = { option_id: optionId };
      if (responder) body.responded_by = responder;
      const response = await this.daemonFetch(`/decisions/${encodeIdSegment(decisionId)}/respond`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body)
      });
      return response.ok;
    }
  }

  /**
   * Reads historical messages matching a pattern.
   */
  async readHistory(pattern, limit = 50) {
    if (this.mode === 'standalone') {
      return this.node.read_history(pattern, limit);
    } else {
      const params = new URLSearchParams({ limit: String(limit), pattern: String(pattern) });
      return this.daemonJson(`/history?${params.toString()}`);
    }
  }

  /**
   * Completely resets the in-memory/persisted states.
   */
  clearState() {
    if (this.mode === 'standalone' && this.node) {
      if (this.saveTimer) {
        clearTimeout(this.saveTimer);
        this.saveTimer = null;
      }
      this.node.clear_state();
      if (this.persistenceKey) {
        try {
          localStorage.removeItem(this.persistenceKey);
        } catch (e) {
          console.warn('Failed to clear MellowMesh state from localStorage:', e);
        }
      }
    }
  }

  /**
   * Cleans up all active connections, timers, and channels. Any pending
   * debounced state write is flushed first.
   */
  close() {
    if (this.saveTimer) {
      this.saveStateLocally();
    }
    this.destroyed = true;
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.socket) {
      this.socket.onclose = null; // Prevent triggering reconnect timer
      this.socket.close();
      this.socket = null;
    }
    if (this.bc) {
      this.bc.close();
      this.bc = null;
    }
  }
}

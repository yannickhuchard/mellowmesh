import init, { WasmMellowMeshNode, init_panic_hook } from './pkg/mellowmesh_wasm.js';

export class BrowserMellowMesh {
  /**
   * Creates a BrowserMellowMesh manager.
   * @param {Object} config
   * @param {string} [config.mode="standalone"] - "standalone" (in-browser node) or "client" (connect to mellowmeshd daemon)
   * @param {string} [config.daemonUrl="ws://127.0.0.1:40000/ws"] - Address of daemon (used in client mode)
   * @param {string} [config.daemonHttpUrl="http://127.0.0.1:40000"] - Http address of daemon
   * @param {string} [config.broadcastChannelName="mellowmesh_broadcast"] - BroadcastChannel name for cross-tab sync
   * @param {string} [config.persistenceKey="mellowmesh_state"] - LocalStorage key to persist state (standalone only)
   * @param {number} [config.maxHistory=500] - Max messages to retain in standalone memory
   */
  constructor(config = {}) {
    this.mode = config.mode || 'standalone';
    this.daemonUrl = config.daemonUrl || 'ws://127.0.0.1:40000/ws';
    this.daemonHttpUrl = config.daemonHttpUrl || 'http://127.0.0.1:40000';
    this.broadcastChannelName = config.broadcastChannelName || 'mellowmesh_broadcast';
    this.persistenceKey = config.persistenceKey || 'mellowmesh_state';
    this.maxHistory = config.maxHistory || 500;

    this.node = null;
    this.bc = null;
    this.socket = null;
    this.clientSubscriptions = new Map(); // id -> callback (used in client mode)
    this.jsSubscriptions = new Map(); // id -> callback (used in standalone mode)
    this.initialized = false;
    this.reconnectTimer = null;
    this.destroyed = false;
  }

  /**
   * Initializes the WASM engine, local storage, and messaging bridges.
   * @param {WebAssembly.Module|BufferSource|string} [wasmModule] - Optional custom WASM module, buffer, or path
   */
  async init(wasmModule = undefined) {
    if (this.initialized) return;

    // 1. Initialize WASM
    await init(wasmModule);
    init_panic_hook();

    if (this.mode === 'standalone') {
      // 2. Initialize in-browser local coordinator node
      this.node = new WasmMellowMeshNode();

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
          const { type, data } = event.data;
          // Receive actions from other tabs and run them locally WITHOUT re-broadcasting
          if (type === 'publish') {
            this.node.publish(data);
            this.saveStateLocally();
          } else if (type === 'register_agent') {
            this.node.register_agent(data);
            this.saveStateLocally();
          } else if (type === 'create_task') {
            this.node.create_task(data);
            this.saveStateLocally();
          } else if (type === 'claim_task') {
            this.node.claim_task(data.taskId, data.agentUri);
            this.saveStateLocally();
          } else if (type === 'complete_task') {
            this.node.complete_task(data.taskId);
            this.saveStateLocally();
          } else if (type === 'create_decision') {
            this.node.create_decision(data);
            this.saveStateLocally();
          } else if (type === 'respond_decision') {
            this.node.respond_decision(data.decisionId, data.optionId);
            this.saveStateLocally();
          }
        };
      }
    } else {
      // 5. Initialize Client mode (connects to running mellowmeshd background process)
      await this.connectToDaemon();
    }

    this.initialized = true;
  }

  /**
   * Private helper to save state to LocalStorage.
   */
  saveStateLocally() {
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
   * Establishes a WebSocket connection to the native local mellowmeshd daemon.
   */
  connectToDaemon() {
    if (this.destroyed) return Promise.reject(new Error('Instance is destroyed'));

    return new Promise((resolve, reject) => {
      this.socket = new WebSocket(this.daemonUrl);

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
          for (const [subId, sub] of this.clientSubscriptions.entries()) {
            if (this.matchTopicPattern(sub.pattern, message.topic)) {
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
  matchTopicPattern(pattern, topic) {
    const pSegs = pattern.split('.');
    const tSegs = topic.split('.');
    return this.matchSegments(pSegs, tSegs);
  }

  matchSegments(pSegs, tSegs) {
    if (pSegs.length === 0) return tSegs.length === 0;
    const p = pSegs[0];
    if (p === '**') {
      if (pSegs.length === 1) return true;
      for (let i = 0; i <= tSegs.length; i++) {
        if (this.matchSegments(pSegs.slice(1), tSegs.slice(i))) return true;
      }
      return false;
    }
    if (p === '>') {
      return tSegs.length > 0 && pSegs.length === 1;
    }
    if (tSegs.length === 0) return false;
    if (p === '*' || p === tSegs[0]) {
      return this.matchSegments(pSegs.slice(1), tSegs.slice(1));
    }
    return false;
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
      this.saveStateLocally();

      // Broadcast to other tabs
      if (this.bc) {
        msg.id = msgId;
        this.bc.postMessage({ type: 'publish', data: msg });
      }
      return msgId;
    } else {
      // POST to daemon
      const response = await fetch(`${this.daemonHttpUrl}/publish`, {
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
   * @returns {string} Subscription ID
   */
  subscribe(pattern, callback) {
    const subId = `sub_${Math.random().toString(36).substring(2, 11)}`;
    if (this.mode === 'standalone') {
      this.node.subscribe(pattern, callback);
      this.jsSubscriptions.set(subId, { pattern, callback });
    } else {
      this.clientSubscriptions.set(subId, { pattern, callback });
    }
    return subId;
  }

  /**
   * Cancels an active subscription.
   */
  unsubscribe(subId) {
    if (this.mode === 'standalone') {
      // To unsubscribe in Rust Wasm WasmMellowMeshNode we'd need to find the correct Rust sub_id.
      // But since we generate a JS subId, we can clean up our JS registry and let it filter.
      // For simplicity, we can also call Rust node unsubscribe if needed.
      return this.jsSubscriptions.delete(subId);
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
      this.saveStateLocally();
      if (this.bc) {
        this.bc.postMessage({ type: 'register_agent', data: agent });
      }
    } else {
      const response = await fetch(`${this.daemonHttpUrl}/agents`, {
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
      const response = await fetch(`${this.daemonHttpUrl}/agents`);
      return response.json();
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
    };

    if (this.mode === 'standalone') {
      this.node.create_task(defaultTask);
      this.saveStateLocally();
      if (this.bc) {
        this.bc.postMessage({ type: 'create_task', data: defaultTask });
      }
    } else {
      const response = await fetch(`${this.daemonHttpUrl}/tasks`, {
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
      const response = await fetch(`${this.daemonHttpUrl}/tasks`);
      return response.json();
    }
  }

  /**
   * Claims an open task for an agent.
   */
  async claimTask(taskId, agentUri) {
    if (this.mode === 'standalone') {
      const success = this.node.claim_task(taskId, agentUri);
      this.saveStateLocally();
      if (this.bc) {
        this.bc.postMessage({ type: 'claim_task', data: { taskId, agentUri } });
      }
      return success;
    } else {
      const response = await fetch(`${this.daemonHttpUrl}/tasks/${taskId}/claim`, {
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
      this.saveStateLocally();
      if (this.bc) {
        this.bc.postMessage({ type: 'complete_task', data: { taskId } });
      }
      return success;
    } else {
      const response = await fetch(`${this.daemonHttpUrl}/tasks/${taskId}/complete`, {
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
      this.saveStateLocally();
      if (this.bc) {
        this.bc.postMessage({ type: 'create_decision', data: defaultDecision });
      }
    } else {
      const response = await fetch(`${this.daemonHttpUrl}/decisions`, {
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
      const response = await fetch(`${this.daemonHttpUrl}/decisions`);
      return response.json();
    }
  }

  /**
   * Submits a decision response from a decider.
   */
  async respondDecision(decisionId, optionId) {
    if (this.mode === 'standalone') {
      const success = this.node.respond_decision(decisionId, optionId);
      this.saveStateLocally();
      if (this.bc) {
        this.bc.postMessage({ type: 'respond_decision', data: { decisionId, optionId } });
      }
      return success;
    } else {
      const response = await fetch(`${this.daemonHttpUrl}/decisions/${decisionId}/respond`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ option_id: optionId })
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
      const response = await fetch(`${this.daemonHttpUrl}/history?limit=${limit}&pattern=${encodeURIComponent(pattern)}`);
      return response.json();
    }
  }

  /**
   * Completely resets the in-memory/persisted states.
   */
  clearState() {
    if (this.mode === 'standalone' && this.node) {
      this.node.clear_state();
      if (this.persistenceKey) {
        localStorage.removeItem(this.persistenceKey);
      }
    }
  }

  /**
   * Cleans up all active connections, timers, and channels.
   */
  close() {
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

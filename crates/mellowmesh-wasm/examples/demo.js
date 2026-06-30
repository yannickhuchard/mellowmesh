import { BrowserMellowMesh } from '../index.js';

let mellowmesh = null;
let activeSimulators = {
  coder: false,
  reviewer: false
};

// Mode switching
window.setMode = async function(mode) {
  document.getElementById('mode-tab-standalone').classList.toggle('active', mode === 'standalone');
  document.getElementById('mode-tab-client').classList.toggle('active', mode === 'client');
  
  const statusIndicator = document.getElementById('status-indicator');
  const statusText = document.getElementById('status-text');
  const launcherPrompt = document.getElementById('launcher-prompt');
  
  if (launcherPrompt) {
    launcherPrompt.style.display = 'none';
  }

  if (mellowmesh) {
    // Clean up active connections, timers, and channels
    mellowmesh.close();
  }

  statusText.textContent = mode === 'standalone' ? 'Connecting WASM...' : 'Connecting Daemon...';
  statusIndicator.className = 'status-dot';

  try {
    mellowmesh = new BrowserMellowMesh({
      mode: mode,
      broadcastChannelName: 'mellowmesh_broadcast',
      persistenceKey: 'mellowmesh_state',
      daemonUrl: 'ws://127.0.0.1:40000/ws',
      daemonHttpUrl: 'http://127.0.0.1:40000'
    });

    await mellowmesh.init();
    
    // Subscribe to all messages for logging
    mellowmesh.subscribe('**', (msg) => {
      appendConsoleLog(msg);
      // Trigger UI updates on core namespaces
      if (msg.topic.startsWith('_task.') || msg.topic.startsWith('_decision.') || msg.topic.startsWith('_agent.')) {
        refreshDashboard();
      }
    });

    statusText.textContent = mode === 'standalone' ? 'Standalone (WASM)' : 'Connected to Daemon';
    statusIndicator.className = 'status-dot';
    
    if (launcherPrompt) {
      launcherPrompt.style.display = 'none';
    }
  } catch (e) {
    console.error('Initialization failed:', e);
    statusText.textContent = mode === 'standalone' ? 'WASM Error' : 'Daemon Offline';
    statusIndicator.className = 'status-dot offline';
    
    if (mode === 'client' && launcherPrompt) {
      launcherPrompt.style.display = 'block';
    }
  }

  refreshDashboard();
};

// Log appending in beautiful developer style
function appendConsoleLog(msg) {
  const consoleLogs = document.getElementById('console-logs');
  if (!consoleLogs) return;

  const entry = document.createElement('div');
  entry.className = 'log-entry';

  const timeStr = new Date(msg.timestamp).toLocaleTimeString();
  
  entry.innerHTML = `
    [${timeStr}] 
    <span class="log-topic">${msg.topic}</span> from 
    <span class="log-from">${msg.from}</span>: 
    <span class="log-body">${msg.body}</span>
  `;

  consoleLogs.appendChild(entry);
  consoleLogs.scrollTop = consoleLogs.scrollHeight;

  // Limit log lines to 100 to keep DOM performing fast
  while (consoleLogs.children.length > 100) {
    consoleLogs.removeChild(consoleLogs.firstChild);
  }
}

// Refresh forms and Kanban boards
async function refreshDashboard() {
  if (!mellowmesh || !mellowmesh.initialized) return;

  try {
    // 1. Fetch tasks
    const tasks = await mellowmesh.listTasks();
    const openList = document.getElementById('tasks-open');
    const claimedList = document.getElementById('tasks-claimed');
    const completedList = document.getElementById('tasks-completed');

    openList.innerHTML = '';
    claimedList.innerHTML = '';
    completedList.innerHTML = '';

    let counts = { open: 0, claimed: 0, completed: 0 };

    tasks.forEach(task => {
      const card = document.createElement('div');
      card.className = 'task-card';
      
      const priorityClass = `badge-priority-${task.priority.toLowerCase()}`;
      const owner = task.claimed_by ? task.claimed_by : `Created by ${task.created_by}`;

      let actionsHtml = '';
      if (task.status === 'open') {
        counts.open++;
        actionsHtml = `
          <div class="task-actions">
            <button class="btn btn-secondary task-btn" onclick="claimTaskAction('${task.id}', 'human://yannick')">Claim</button>
          </div>
        `;
        openList.appendChild(card);
      } else if (task.status === 'claimed') {
        counts.claimed++;
        actionsHtml = `
          <div class="task-actions">
            <button class="btn btn-secondary task-btn" onclick="completeTaskAction('${task.id}')">Complete</button>
          </div>
        `;
        claimedList.appendChild(card);
      } else {
        counts.completed++;
        completedList.appendChild(card);
      }

      card.innerHTML = `
        <div class="task-card-title">${task.title}</div>
        <div class="task-card-desc">${task.description || 'No description provided.'}</div>
        <div class="task-card-meta">
          <span class="badge ${priorityClass}">${task.priority.toUpperCase()}</span>
          <span class="task-card-owner">${owner}</span>
        </div>
        ${actionsHtml}
      `;
    });

    document.getElementById('col-open-count').textContent = counts.open;
    document.getElementById('col-claimed-count').textContent = counts.claimed;
    document.getElementById('col-completed-count').textContent = counts.completed;

    // 2. Fetch decisions
    const decisions = await mellowmesh.listDecisions();
    const decisionsContainer = document.getElementById('decisions-container');
    decisionsContainer.innerHTML = '';

    if (decisions.length === 0) {
      decisionsContainer.innerHTML = `
        <div style="font-size: 0.85rem; color: var(--text-muted); text-align: center; padding: 1rem 0;">
          No decisions requiring human review.
        </div>
      `;
    } else {
      decisions.forEach(dec => {
        const card = document.createElement('div');
        card.className = 'decision-card';
        
        let optionsHtml = '';
        if (dec.response_option_id) {
          optionsHtml = `<div class="decision-responded">✓ Responded with Option: <strong>${dec.response_option_id}</strong></div>`;
        } else {
          optionsHtml = `
            <div class="decision-options">
              ${dec.options.map(opt => `
                <button class="decision-btn" onclick="respondDecisionAction('${dec.id}', '${opt.id}')">${opt.label}</button>
              `).join('')}
            </div>
          `;
        }

        card.innerHTML = `
          <div class="decision-header">
            <div class="decision-title">${dec.title}</div>
            <span style="font-size:0.7rem; color:var(--text-muted);">Required Decider: ${dec.required_decider}</span>
          </div>
          <div class="decision-question">${dec.question}</div>
          ${optionsHtml}
        `;
        decisionsContainer.appendChild(card);
      });
    }
  } catch (e) {
    console.error('Error refreshing dashboard:', e);
  }
}

// Form Handlers
window.registerAgentSubmit = async function() {
  const name = document.getElementById('agent-name').value;
  const owner = document.getElementById('agent-owner').value;
  const cap = document.getElementById('agent-capability').value;

  const agentId = `agent://${owner.replace('human://', '')}/${name.toLowerCase().replace(/\s+/g, '-')}`;
  
  const agent = {
    id: agentId,
    name: name,
    owner: owner,
    mode: 'autonomous',
    capabilities: [cap]
  };

  try {
    await mellowmesh.registerAgent(agent);
    document.getElementById('agent-form').reset();
    document.getElementById('agent-owner').value = 'human://yannick';
  } catch (e) {
    alert('Failed to register agent: ' + e.message);
  }
};

window.createTaskSubmit = async function() {
  const title = document.getElementById('task-title').value;
  const topic = document.getElementById('task-topic').value;
  const cap = document.getElementById('task-cap').value;
  const priority = document.getElementById('task-priority').value;
  const desc = document.getElementById('task-desc').value;

  const task = {
    title,
    topics: [topic],
    required_capabilities: [cap],
    priority,
    description: desc,
    created_by: 'human://yannick',
    status: 'open',
    artifacts: [],
    decisions: []
  };

  try {
    await mellowmesh.createTask(task);
    document.getElementById('task-form').reset();
    document.getElementById('task-topic').value = '_task.code.wasm';
    document.getElementById('task-priority').value = 'medium';
  } catch (e) {
    alert('Failed to create task: ' + e.message);
  }
};

window.sandboxPublishSubmit = async function() {
  const topic = document.getElementById('sb-topic').value;
  const from = document.getElementById('sb-from').value;
  const body = document.getElementById('sb-body').value;

  const msg = {
    topic,
    from,
    body,
    timestamp: new Date().toISOString()
  };

  try {
    await mellowmesh.publish(msg);
    document.getElementById('sb-body').value = '';
  } catch (e) {
    alert('Failed to publish message: ' + e.message);
  }
};

// UI Button Actions
window.claimTaskAction = async function(taskId, agentUri) {
  try {
    await mellowmesh.claimTask(taskId, agentUri);
  } catch (e) {
    console.error(e);
  }
};

window.completeTaskAction = async function(taskId) {
  try {
    await mellowmesh.completeTask(taskId);
  } catch (e) {
    console.error(e);
  }
};

window.respondDecisionAction = async function(decisionId, optionId) {
  try {
    await mellowmesh.respondDecision(decisionId, optionId);
  } catch (e) {
    console.error(e);
  }
};

window.resetState = function() {
  if (confirm('Are you sure you want to wipe all local coordination data?')) {
    if (mellowmesh) {
      mellowmesh.clearState();
    }
    window.location.reload();
  }
};

// ========================================================
// SIMULATED AUTONOMOUS AGENT loops
// ========================================================

window.toggleSimulators = function() {
  activeSimulators.coder = document.getElementById('sim-coder').checked;
  activeSimulators.reviewer = document.getElementById('sim-reviewer').checked;
};

// Background polling loop simulating agent cognitive loops
async function runSimulatorsLoop() {
  if (!mellowmesh || !mellowmesh.initialized) {
    setTimeout(runSimulatorsLoop, 1000);
    return;
  }

  try {
    const tasks = await mellowmesh.listTasks();
    
    // 1. Coder Simulator Loop
    if (activeSimulators.coder) {
      const openCodeTask = tasks.find(t => t.status === 'open' && t.required_capabilities.includes('code.write'));
      if (openCodeTask) {
        const agentUri = 'agent://yannick/coder';
        console.log(`[Simulator Coder] Claiming task: ${openCodeTask.title}`);
        
        // Claim the task
        await mellowmesh.claimTask(openCodeTask.id, agentUri);
        
        // Simulate thinking & writing code
        setTimeout(async () => {
          await mellowmesh.publish({
            topic: `_agent.coder.status`,
            from: agentUri,
            body: `Analyzing requirements for '${openCodeTask.title}'...`
          });
          
          setTimeout(async () => {
            await mellowmesh.publish({
              topic: `_agent.coder.status`,
              from: agentUri,
              body: `Wrote WebAssembly export bindings. Requesting review.`
            });

            // Create a decision request for approval
            await mellowmesh.createDecision({
              title: `Deploy code for task '${openCodeTask.title}'`,
              question: `Does the compiled WASM interface for '${openCodeTask.title}' look correct?`,
              created_by: agentUri,
              required_decider: 'human://yannick',
              options: [
                { id: 'approve', label: 'Approve & Merge' },
                { id: 'reject', label: 'Request Changes' }
              ]
            });
            
            // Start checking for decision response
            pollForDecisionResponse(openCodeTask.id, agentUri);
          }, 3000);
        }, 1500);
      }
    }

    // 2. Reviewer Simulator Loop
    if (activeSimulators.reviewer) {
      const openReviewTask = tasks.find(t => t.status === 'open' && t.required_capabilities.includes('code.review'));
      if (openReviewTask) {
        const agentUri = 'agent://yannick/reviewer';
        console.log(`[Simulator Reviewer] Claiming task: ${openReviewTask.title}`);
        
        await mellowmesh.claimTask(openReviewTask.id, agentUri);
        
        setTimeout(async () => {
          await mellowmesh.publish({
            topic: `_agent.reviewer.status`,
            from: agentUri,
            body: `Reviewing task: ${openReviewTask.title}. Code looks clean, tests are passing.`
          });
          
          setTimeout(async () => {
            await mellowmesh.completeTask(openReviewTask.id);
          }, 2000);
        }, 2000);
      }
    }
  } catch (e) {
    console.error('Error in simulation tick:', e);
  }

  // Next tick in 4 seconds
  setTimeout(runSimulatorsLoop, 4000);
}

// Helper to poll for a decision approval and then complete task
async function pollForDecisionResponse(taskId, agentUri) {
  try {
    const decisions = await mellowmesh.listDecisions();
    // Find decision associated with this agent's request
    const matchingDec = decisions.find(d => d.created_by === agentUri && !d.response_option_id === false);
    
    if (matchingDec && matchingDec.response_option_id) {
      if (matchingDec.response_option_id === 'approve') {
        await mellowmesh.publish({
          topic: `_agent.coder.status`,
          from: agentUri,
          body: `Code approved by decider. Completing task.`
        });
        await mellowmesh.completeTask(taskId);
      } else {
        await mellowmesh.publish({
          topic: `_agent.coder.status`,
          from: agentUri,
          body: `Code changes requested. Resetting task status to open.`
        });
        // Remove task assignment / reopen
        const tasks = await mellowmesh.listTasks();
        const t = tasks.find(x => x.id === taskId);
        if (t) {
          t.status = 'open';
          t.claimed_by = null;
          await mellowmesh.createTask(t); // Update task
        }
      }
    } else {
      // Keep polling if not resolved
      setTimeout(() => pollForDecisionResponse(taskId, agentUri), 2000);
    }
  } catch (e) {
    console.error(e);
  }
}

// Start everything up on load
window.addEventListener('DOMContentLoaded', () => {
  // Default to standalone WASM mode
  window.setMode('standalone');
  // Run agent loop in background
  runSimulatorsLoop();
});

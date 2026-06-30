import fs from 'fs';
import path from 'path';
import assert from 'assert';
import { fileURLToPath } from 'url';
import { BroadcastChannel } from 'worker_threads';
import { BrowserMellowMesh } from '../index.js';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Polyfill BroadcastChannel if not available
if (typeof global.BroadcastChannel === 'undefined') {
  global.BroadcastChannel = BroadcastChannel;
}

// Polyfill localStorage to prevent warning logs
global.localStorage = {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {}
};

// Mock WebSocket class to simulate mellowmeshd native app
class MockWebSocket {
  static instances = [];
  static onConnectionCallback = null;

  constructor(url) {
    this.url = url;
    this.onopen = null;
    this.onmessage = null;
    this.onerror = null;
    this.onclose = null;
    this.readyState = 0; // CONNECTING
    MockWebSocket.instances.push(this);

    // Simulate async connection success
    setTimeout(() => {
      this.readyState = 1; // OPEN
      if (this.onopen) this.onopen();
      if (MockWebSocket.onConnectionCallback) {
        MockWebSocket.onConnectionCallback(this);
      }
    }, 10);
  }

  send(data) {
    // Send message to virtual server
    if (this.onServerMessage) {
      this.onServerMessage(data);
    }
  }

  close() {
    this.readyState = 3; // CLOSED
    if (this.onclose) this.onclose();
  }

  // Simulate server sending message to client
  serverSend(data) {
    if (this.onmessage) {
      this.onmessage({ data });
    }
  }
}

// Mock global WebSocket
global.WebSocket = MockWebSocket;

// Mock global fetch to simulate HTTP REST endpoints of mellowmeshd daemon
const mockTasksDb = [];
global.fetch = async (url, options = {}) => {
  const parsedUrl = new URL(url);
  
  if (parsedUrl.pathname === '/publish' && options.method === 'POST') {
    const body = JSON.parse(options.body);
    // Broadcast message to all mock WebSocket clients
    MockWebSocket.instances.forEach(ws => {
      ws.serverSend(JSON.stringify(body));
    });
    return {
      ok: true,
      text: async () => body.id
    };
  }

  if (parsedUrl.pathname === '/tasks' && options.method === 'POST') {
    const body = JSON.parse(options.body);
    mockTasksDb.push(body);
    // Broadcast task system message to all clients
    const systemMsg = {
      id: 'system_msg_1',
      topic: '_task.system.created',
      from: 'system://coordinator',
      timestamp: new Date().toISOString(),
      body: `Task created: ${body.title}`
    };
    MockWebSocket.instances.forEach(ws => {
      ws.serverSend(JSON.stringify(systemMsg));
    });
    return { ok: true };
  }

  if (parsedUrl.pathname === '/tasks' && options.method === 'GET') {
    return {
      ok: true,
      json: async () => mockTasksDb
    };
  }

  return { ok: false, statusText: 'Not Found' };
};

async function runTests() {
  console.log('--------------------------------------------------');
  console.log('Running MellowMesh Browser Communications Test Suite');
  console.log('--------------------------------------------------');

  // Load compiled WASM buffer
  const wasmPath = path.join(__dirname, '../pkg/mellowmesh_wasm_bg.wasm');
  const wasmBuffer = fs.readFileSync(wasmPath);



  // ========================================================
  // Scenario 1: Tab-to-Tab (Same Origin BroadcastChannel)
  // ========================================================
  console.log('Scenario 1: Testing Browser Tab-to-Tab Exchange (BroadcastChannel)...');
  
  const tab1 = new BrowserMellowMesh({
    mode: 'standalone',
    broadcastChannelName: 'test_tab_sync',
    persistenceKey: null // No local storage write
  });

  const tab2 = new BrowserMellowMesh({
    mode: 'standalone',
    broadcastChannelName: 'test_tab_sync',
    persistenceKey: null
  });

  await tab1.init(wasmBuffer);
  await tab2.init(wasmBuffer);

  const receivedMessages = [];
  tab2.subscribe('_forum.**', (msg) => {
    receivedMessages.push(msg);
  });

  // Tab 1 publishes a message
  await tab1.publish({
    topic: '_forum.general',
    from: 'human://yannick',
    body: 'Hello from Tab 1!'
  });

  // Wait for BroadcastChannel async delivery
  await new Promise(resolve => setTimeout(resolve, 50));

  assert.strictEqual(receivedMessages.length, 1, 'Tab 2 should receive the broadcasted message');
  assert.strictEqual(receivedMessages[0].body, 'Hello from Tab 1!');
  assert.strictEqual(receivedMessages[0].topic, '_forum.general');
  console.log('✓ Scenario 1 passed successfully.');

  // Clean up
  tab1.bc.close();
  tab2.bc.close();

  // ========================================================
  // Scenario 2: Browser to App (Client WebSocket Connection)
  // ========================================================
  console.log('\nScenario 2: Testing Browser to App Client Connection...');

  MockWebSocket.instances = []; // Reset instances

  const clientTab = new BrowserMellowMesh({
    mode: 'client',
    daemonUrl: 'ws://127.0.0.1:40000/ws',
    daemonHttpUrl: 'http://127.0.0.1:40000'
  });

  await clientTab.init(wasmBuffer);

  // Verify client opened WebSocket connection
  assert.strictEqual(MockWebSocket.instances.length, 1, 'Client should connect to mock WebSocket server');
  
  // Publish message via client
  const publishPromise = clientTab.publish({
    topic: '_forum.support',
    from: 'human://yannick',
    body: 'Need help'
  });

  await publishPromise;
  console.log('✓ Scenario 2 passed successfully.');

  // ========================================================
  // Scenario 3: Browser Tab-to-Tab to App (Hybrid WebSocket Routing)
  // ========================================================
  console.log('\nScenario 3: Testing Tab-to-Tab to App Routing (Broker broadcast)...');

  MockWebSocket.instances = []; // Reset instances

  const clientTab1 = new BrowserMellowMesh({
    mode: 'client',
    daemonUrl: 'ws://127.0.0.1:40000/ws',
    daemonHttpUrl: 'http://127.0.0.1:40000'
  });

  const clientTab2 = new BrowserMellowMesh({
    mode: 'client',
    daemonUrl: 'ws://127.0.0.1:40000/ws',
    daemonHttpUrl: 'http://127.0.0.1:40000'
  });

  await clientTab1.init(wasmBuffer);
  await clientTab2.init(wasmBuffer);

  assert.strictEqual(MockWebSocket.instances.length, 2, 'Both client tabs should connect to WebSocket server');

  const tab2Received = [];
  clientTab2.subscribe('_task.**', (msg) => {
    tab2Received.push(msg);
  });

  // Client Tab 1 creates a task
  // In our global.fetch mock, creating a task triggers a broadcast system message '_task.system.created'
  // back to all connected client WebSockets.
  await clientTab1.createTask({
    title: 'WASM Peer Test',
    created_by: 'human://yannick'
  });

  // Wait for mock server socket broadcast
  await new Promise(resolve => setTimeout(resolve, 50));

  assert.strictEqual(tab2Received.length, 1, 'Client Tab 2 should receive routed task system message');
  assert.strictEqual(tab2Received[0].topic, '_task.system.created');
  assert.ok(tab2Received[0].body.includes('WASM Peer Test'));
  console.log('✓ Scenario 3 passed successfully.');

  console.log('\n--------------------------------------------------');
  console.log('All Browser Communication scenarios passed successfully!');
  console.log('--------------------------------------------------');
}

runTests().catch(err => {
  console.error('Test failed with error:', err);
  process.exit(1);
});

use chrono::Utc;
use mellowmesh_core::agent::AgentRegistration;
use mellowmesh_core::decision::Decision;
use mellowmesh_core::message::Message;
use mellowmesh_core::task::Task;
use mellowmesh_core::topic::match_topic;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use ulid::Ulid;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

struct Subscription {
    id: String,
    pattern: String,
    callback: js_sys::Function,
}

#[derive(Serialize, Deserialize)]
struct FullState {
    messages: Vec<Message>,
    agents: Vec<AgentRegistration>,
    tasks: Vec<Task>,
    decisions: Vec<Decision>,
}

#[wasm_bindgen]
pub struct WasmMellowMeshNode {
    messages: Mutex<Vec<Message>>,
    agents: Mutex<Vec<AgentRegistration>>,
    tasks: Mutex<Vec<Task>>,
    decisions: Mutex<Vec<Decision>>,
    subscriptions: Mutex<Vec<Subscription>>,
}

#[wasm_bindgen]
impl WasmMellowMeshNode {
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmMellowMeshNode {
        WasmMellowMeshNode {
            messages: Mutex::new(Vec::new()),
            agents: Mutex::new(Vec::new()),
            tasks: Mutex::new(Vec::new()),
            decisions: Mutex::new(Vec::new()),
            subscriptions: Mutex::new(Vec::new()),
        }
    }

    pub fn publish(&self, message_val: JsValue) -> Result<String, JsValue> {
        let mut msg: Message = serde_wasm_bindgen::from_value(message_val)
            .map_err(|e| JsValue::from_str(&format!("Failed to parse message: {}", e)))?;

        // Generate ULID if ID is empty
        if msg.id.is_empty() {
            msg.id = format!("msg_{}", Ulid::new().to_string().to_lowercase());
        }

        // Standardize timestamp if not set
        if msg.timestamp.timestamp_millis() == 0 {
            msg.timestamp = Utc::now();
        }

        // Save message inside scoped block to drop lock immediately
        {
            let mut messages = self.messages.lock().unwrap();
            messages.push(msg.clone());
        }

        // Gather matching subscription callbacks and drop subscriptions lock before executing them
        let matching_subs: Vec<js_sys::Function> = {
            let subs = self.subscriptions.lock().unwrap();
            subs.iter()
                .filter(|sub| match_topic(&sub.pattern, &msg.topic))
                .map(|sub| sub.callback.clone())
                .collect()
        };

        let js_msg = serde_wasm_bindgen::to_value(&msg)
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize message: {}", e)))?;

        for callback in matching_subs {
            let this = JsValue::null();
            let _ = callback.call1(&this, &js_msg);
        }

        Ok(msg.id)
    }

    pub fn subscribe(&self, pattern: String, callback: js_sys::Function) -> String {
        let sub_id = format!("sub_{}", Ulid::new().to_string().to_lowercase());
        let mut subs = self.subscriptions.lock().unwrap();
        subs.push(Subscription {
            id: sub_id.clone(),
            pattern,
            callback,
        });
        sub_id
    }

    pub fn unsubscribe(&self, sub_id: String) -> bool {
        let mut subs = self.subscriptions.lock().unwrap();
        let initial_len = subs.len();
        subs.retain(|sub| sub.id != sub_id);
        subs.len() < initial_len
    }

    pub fn register_agent(&self, agent_val: JsValue) -> Result<(), JsValue> {
        let agent: AgentRegistration = serde_wasm_bindgen::from_value(agent_val)
            .map_err(|e| JsValue::from_str(&format!("Failed to parse AgentRegistration: {}", e)))?;

        {
            let mut agents = self.agents.lock().unwrap();
            agents.retain(|a| a.id != agent.id);
            agents.push(agent.clone());
        } // Lock released

        // Publish system message
        let system_msg = Message {
            id: format!("msg_{}", Ulid::new().to_string().to_lowercase()),
            topic: format!(
                "_agent.{}.status",
                agent.id.replace("agent://", "").replace("/", ".")
            ),
            from: "system://coordinator".to_string(),
            owner: None,
            timestamp: Utc::now(),
            content_type: "application/json".to_string(),
            body: format!("Agent {} registered.", agent.name),
            headers: None,
            payload: Some(serde_json::to_value(&agent).unwrap_or(serde_json::Value::Null)),
            parent_id: None,
        };
        let _ = self.publish(serde_wasm_bindgen::to_value(&system_msg)?);

        Ok(())
    }

    pub fn list_agents(&self) -> Result<JsValue, JsValue> {
        let agents = self.agents.lock().unwrap();
        serde_wasm_bindgen::to_value(&*agents)
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize agents: {}", e)))
    }

    pub fn create_task(&self, task_val: JsValue) -> Result<(), JsValue> {
        let mut task: Task = serde_wasm_bindgen::from_value(task_val)
            .map_err(|e| JsValue::from_str(&format!("Failed to parse Task: {}", e)))?;

        if task.id.is_empty() {
            task.id = format!("task_{}", Ulid::new().to_string().to_lowercase());
        }

        {
            let mut tasks = self.tasks.lock().unwrap();
            tasks.retain(|t| t.id != task.id);
            tasks.push(task.clone());
        } // Lock released

        // Publish task system message
        let system_msg = Message {
            id: format!("msg_{}", Ulid::new().to_string().to_lowercase()),
            topic: format!("_task.system.created"),
            from: "system://coordinator".to_string(),
            owner: None,
            timestamp: Utc::now(),
            content_type: "application/json".to_string(),
            body: format!("Task '{}' created.", task.title),
            headers: None,
            payload: Some(serde_json::to_value(&task).unwrap_or(serde_json::Value::Null)),
            parent_id: None,
        };
        let _ = self.publish(serde_wasm_bindgen::to_value(&system_msg)?);

        Ok(())
    }

    pub fn list_tasks(&self) -> Result<JsValue, JsValue> {
        let tasks = self.tasks.lock().unwrap();
        serde_wasm_bindgen::to_value(&*tasks)
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize tasks: {}", e)))
    }

    pub fn claim_task(&self, task_id: String, agent_uri: String) -> Result<bool, JsValue> {
        let mut system_msg = None;
        let success = {
            let mut tasks = self.tasks.lock().unwrap();
            if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
                task.status = "claimed".to_string();
                task.claimed_by = Some(agent_uri.clone());

                system_msg = Some(Message {
                    id: format!("msg_{}", Ulid::new().to_string().to_lowercase()),
                    topic: format!("_task.{}.claimed", task_id),
                    from: agent_uri.clone(),
                    owner: None,
                    timestamp: Utc::now(),
                    content_type: "application/json".to_string(),
                    body: format!("Task '{}' claimed by {}.", task.title, agent_uri),
                    headers: None,
                    payload: Some(serde_json::to_value(&task).unwrap_or(serde_json::Value::Null)),
                    parent_id: None,
                });
                true
            } else {
                false
            }
        }; // Lock released

        if let Some(msg) = system_msg {
            let _ = self.publish(serde_wasm_bindgen::to_value(&msg)?);
        }

        Ok(success)
    }

    pub fn complete_task(&self, task_id: String) -> Result<bool, JsValue> {
        let mut system_msg = None;
        let success = {
            let mut tasks = self.tasks.lock().unwrap();
            if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
                task.status = "completed".to_string();

                system_msg = Some(Message {
                    id: format!("msg_{}", Ulid::new().to_string().to_lowercase()),
                    topic: format!("_task.{}.completed", task_id),
                    from: task
                        .claimed_by
                        .clone()
                        .unwrap_or_else(|| "system://coordinator".to_string()),
                    owner: None,
                    timestamp: Utc::now(),
                    content_type: "application/json".to_string(),
                    body: format!("Task '{}' completed.", task.title),
                    headers: None,
                    payload: Some(serde_json::to_value(&task).unwrap_or(serde_json::Value::Null)),
                    parent_id: None,
                });
                true
            } else {
                false
            }
        }; // Lock released

        if let Some(msg) = system_msg {
            let _ = self.publish(serde_wasm_bindgen::to_value(&msg)?);
        }

        Ok(success)
    }

    pub fn create_decision(&self, decision_val: JsValue) -> Result<(), JsValue> {
        let mut decision: Decision = serde_wasm_bindgen::from_value(decision_val)
            .map_err(|e| JsValue::from_str(&format!("Failed to parse Decision: {}", e)))?;

        if decision.id.is_empty() {
            decision.id = format!("decision_{}", Ulid::new().to_string().to_lowercase());
        }

        {
            let mut decisions = self.decisions.lock().unwrap();
            decisions.retain(|d| d.id != decision.id);
            decisions.push(decision.clone());
        } // Lock released

        // Publish system message
        let system_msg = Message {
            id: format!("msg_{}", Ulid::new().to_string().to_lowercase()),
            topic: format!("_decision.system.created"),
            from: "system://coordinator".to_string(),
            owner: None,
            timestamp: Utc::now(),
            content_type: "application/json".to_string(),
            body: format!("Decision Request: '{}' created.", decision.title),
            headers: None,
            payload: Some(serde_json::to_value(&decision).unwrap_or(serde_json::Value::Null)),
            parent_id: None,
        };
        let _ = self.publish(serde_wasm_bindgen::to_value(&system_msg)?);

        Ok(())
    }

    pub fn list_decisions(&self) -> Result<JsValue, JsValue> {
        let decisions = self.decisions.lock().unwrap();
        serde_wasm_bindgen::to_value(&*decisions)
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize decisions: {}", e)))
    }

    pub fn respond_decision(
        &self,
        decision_id: String,
        option_id: String,
    ) -> Result<bool, JsValue> {
        let mut system_msg = None;
        let success = {
            let mut decisions = self.decisions.lock().unwrap();
            if let Some(decision) = decisions.iter_mut().find(|d| d.id == decision_id) {
                decision.status = "approved".to_string();
                decision.response_option_id = Some(option_id.clone());
                decision.response_timestamp = Some(Utc::now());

                system_msg = Some(Message {
                    id: format!("msg_{}", Ulid::new().to_string().to_lowercase()),
                    topic: format!("_decision.{}.responded", decision_id),
                    from: decision.required_decider.clone(),
                    owner: None,
                    timestamp: Utc::now(),
                    content_type: "application/json".to_string(),
                    body: format!(
                        "Decision '{}' responded with option {}.",
                        decision.title, option_id
                    ),
                    headers: None,
                    payload: Some(
                        serde_json::to_value(&decision).unwrap_or(serde_json::Value::Null),
                    ),
                    parent_id: None,
                });
                true
            } else {
                false
            }
        }; // Lock released

        if let Some(msg) = system_msg {
            let _ = self.publish(serde_wasm_bindgen::to_value(&msg)?);
        }

        Ok(success)
    }

    pub fn read_history(&self, pattern: String, limit: usize) -> Result<JsValue, JsValue> {
        let messages = self.messages.lock().unwrap();
        let mut matching: Vec<Message> = messages
            .iter()
            .filter(|msg| match_topic(&pattern, &msg.topic))
            .cloned()
            .collect();

        matching.sort_by_key(|m| m.timestamp);

        let len = matching.len();
        let start = if len > limit { len - limit } else { 0 };
        let slice = &matching[start..];

        serde_wasm_bindgen::to_value(slice)
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize history: {}", e)))
    }

    pub fn clear_state(&self) {
        self.messages.lock().unwrap().clear();
        self.agents.lock().unwrap().clear();
        self.tasks.lock().unwrap().clear();
        self.decisions.lock().unwrap().clear();
    }

    pub fn get_state(&self) -> Result<JsValue, JsValue> {
        let state = FullState {
            messages: self.messages.lock().unwrap().clone(),
            agents: self.agents.lock().unwrap().clone(),
            tasks: self.tasks.lock().unwrap().clone(),
            decisions: self.decisions.lock().unwrap().clone(),
        };
        serde_wasm_bindgen::to_value(&state)
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize state: {}", e)))
    }

    pub fn load_state(&self, state_val: JsValue) -> Result<(), JsValue> {
        let state: FullState = serde_wasm_bindgen::from_value(state_val)
            .map_err(|e| JsValue::from_str(&format!("Failed to parse state: {}", e)))?;

        *self.messages.lock().unwrap() = state.messages;
        *self.agents.lock().unwrap() = state.agents;
        *self.tasks.lock().unwrap() = state.tasks;
        *self.decisions.lock().unwrap() = state.decisions;

        Ok(())
    }
}

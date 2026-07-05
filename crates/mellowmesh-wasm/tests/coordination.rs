use mellowmesh_core::decision::{Decision, DecisionOption};
use mellowmesh_core::message::Message;
use mellowmesh_core::task::Task;
use mellowmesh_wasm::WasmMellowMeshNode;
use std::sync::{Arc, Mutex};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

#[wasm_bindgen_test]
fn test_node_initialization() {
    let node = WasmMellowMeshNode::new();
    let tasks_val = node.list_tasks().unwrap();
    let tasks: Vec<Task> = serde_wasm_bindgen::from_value(tasks_val).unwrap();
    assert_eq!(tasks.len(), 0);
}

#[wasm_bindgen_test]
fn test_pub_sub_wildcards() {
    let node = WasmMellowMeshNode::new();
    let counter = Arc::new(Mutex::new(0));

    // Subscribe to task wildcard
    let counter_clone = counter.clone();
    let callback = Closure::wrap(Box::new(move |_msg: JsValue| {
        let mut val = counter_clone.lock().unwrap();
        *val += 1;
    }) as Box<dyn FnMut(JsValue)>);

    let js_func = callback
        .into_js_value()
        .unchecked_into::<js_sys::Function>();
    let sub_id = node.subscribe("_task.**".to_string(), js_func);

    // Publish matching message
    let matching_msg = Message {
        id: "".to_string(),
        topic: "_task.code.claim".to_string(),
        from: "agent://coder".to_string(),
        owner: None,
        timestamp: chrono::Utc::now(),
        content_type: "text/plain".to_string(),
        body: "Claimed".to_string(),
        headers: None,
        payload: None,
        parent_id: None,
    };
    node.publish(serde_wasm_bindgen::to_value(&matching_msg).unwrap())
        .unwrap();

    // Publish non-matching message
    let non_matching_msg = Message {
        id: "".to_string(),
        topic: "_forum.general".to_string(),
        from: "human://yannick".to_string(),
        owner: None,
        timestamp: chrono::Utc::now(),
        content_type: "text/plain".to_string(),
        body: "Hello".to_string(),
        headers: None,
        payload: None,
        parent_id: None,
    };
    node.publish(serde_wasm_bindgen::to_value(&non_matching_msg).unwrap())
        .unwrap();

    // Counter should be 1 (only matching message triggered it)
    assert_eq!(*counter.lock().unwrap(), 1);

    // Unsubscribe
    assert!(node.unsubscribe(sub_id));

    // Publish matching message again
    node.publish(serde_wasm_bindgen::to_value(&matching_msg).unwrap())
        .unwrap();

    // Counter should still be 1 after unsubscribe
    assert_eq!(*counter.lock().unwrap(), 1);
}

#[wasm_bindgen_test]
fn test_task_lifecycle() {
    let node = WasmMellowMeshNode::new();

    let task = Task {
        id: "task_test".to_string(),
        title: "Test Task".to_string(),
        description: None,
        created_from: None,
        created_by: "human://yannick".to_string(),
        status: "open".to_string(),
        priority: "medium".to_string(),
        topics: vec!["_task.test".to_string()],
        required_capabilities: vec!["test".to_string()],
        assigned_to: None,
        claimed_by: None,
        deadline: None,
        artifacts: vec![],
        decisions: vec![],
        parent_id: None,
        lease_seconds: None,
        claim_expires_at: None,
    };

    node.create_task(serde_wasm_bindgen::to_value(&task).unwrap())
        .unwrap();

    // Verify task is added
    let tasks_val = node.list_tasks().unwrap();
    let tasks: Vec<Task> = serde_wasm_bindgen::from_value(tasks_val).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "task_test");
    assert_eq!(tasks[0].status, "open");

    // Claim task
    let claimed = node
        .claim_task("task_test".to_string(), "agent://tester".to_string())
        .unwrap();
    assert!(claimed);

    let tasks_val = node.list_tasks().unwrap();
    let tasks: Vec<Task> = serde_wasm_bindgen::from_value(tasks_val).unwrap();
    assert_eq!(tasks[0].status, "claimed");
    assert_eq!(tasks[0].claimed_by, Some("agent://tester".to_string()));

    // Complete task
    let completed = node.complete_task("task_test".to_string()).unwrap();
    assert!(completed);

    let tasks_val = node.list_tasks().unwrap();
    let tasks: Vec<Task> = serde_wasm_bindgen::from_value(tasks_val).unwrap();
    assert_eq!(tasks[0].status, "completed");
}

#[wasm_bindgen_test]
fn test_decision_consensus() {
    let node = WasmMellowMeshNode::new();

    let decision = Decision {
        id: "dec_test".to_string(),
        title: "Test Decision".to_string(),
        question: "Approve changes?".to_string(),
        created_by: "agent://tester".to_string(),
        required_decider: "human://yannick".to_string(),
        status: "requested".to_string(),
        options: vec![
            DecisionOption {
                id: "yes".to_string(),
                label: "Yes".to_string(),
                pros: vec![],
                cons: vec![],
            },
            DecisionOption {
                id: "no".to_string(),
                label: "No".to_string(),
                pros: vec![],
                cons: vec![],
            },
        ],
        response_option_id: None,
        response_timestamp: None,
    };

    node.create_decision(serde_wasm_bindgen::to_value(&decision).unwrap())
        .unwrap();

    // Verify decision listed
    let decs_val = node.list_decisions().unwrap();
    let decs: Vec<Decision> = serde_wasm_bindgen::from_value(decs_val).unwrap();
    assert_eq!(decs.len(), 1);
    assert_eq!(decs[0].status, "requested");

    // Respond to decision
    let responded = node
        .respond_decision("dec_test".to_string(), "yes".to_string())
        .unwrap();
    assert!(responded);

    let decs_val = node.list_decisions().unwrap();
    let decs: Vec<Decision> = serde_wasm_bindgen::from_value(decs_val).unwrap();
    assert_eq!(decs[0].status, "approved");
    assert_eq!(decs[0].response_option_id, Some("yes".to_string()));
    assert!(decs[0].response_timestamp.is_some());
}

#[wasm_bindgen_test]
fn test_state_serialization() {
    let node = WasmMellowMeshNode::new();

    // Create a task
    let task = Task {
        id: "task_persist".to_string(),
        title: "Persist Task".to_string(),
        description: None,
        created_from: None,
        created_by: "human://yannick".to_string(),
        status: "open".to_string(),
        priority: "low".to_string(),
        topics: vec![],
        required_capabilities: vec![],
        assigned_to: None,
        claimed_by: None,
        deadline: None,
        artifacts: vec![],
        decisions: vec![],
        parent_id: None,
        lease_seconds: None,
        claim_expires_at: None,
    };
    node.create_task(serde_wasm_bindgen::to_value(&task).unwrap())
        .unwrap();

    // Get current state
    let state_val = node.get_state().unwrap();

    // Clear state
    node.clear_state();
    let tasks_val = node.list_tasks().unwrap();
    let tasks: Vec<Task> = serde_wasm_bindgen::from_value(tasks_val).unwrap();
    assert_eq!(tasks.len(), 0);

    // Reload state
    node.load_state(state_val).unwrap();
    let tasks_val = node.list_tasks().unwrap();
    let tasks: Vec<Task> = serde_wasm_bindgen::from_value(tasks_val).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "task_persist");
}

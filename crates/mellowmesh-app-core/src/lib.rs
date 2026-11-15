//! # mellowmesh-app-core
//!
//! The shared Rust core of the native MellowMesh apps — SwiftUI (iOS +
//! macOS), Jetpack Compose (Android) and WinUI 3 (Windows) — exposed through
//! [UniFFI](https://mozilla.github.io/uniffi-rs/) proc-macros. The shells only
//! draw; connection, pairing validation, view models, conversation
//! derivation, live events, MeshUI validation and the Mesh graph layout all
//! live here. See `README.md` for the API, topic conventions and the
//! per-platform build recipes.

uniffi::setup_scaffolding!();

pub mod conversation;
pub mod error;
pub mod events;
pub mod graph;
pub mod identity;
pub mod meshui;
pub mod model;
pub mod pairing;
pub mod presence;
pub mod session;
mod snapshot;
mod transport;

pub use conversation::{
    conversation_for_topic, dm_conversation_id, ConversationKind, ConversationRef,
};
pub use error::MeshError;
pub use events::{MeshEvent, MeshEventListener};
pub use graph::{
    layout_graph, GraphEdge, GraphEdgeKind, GraphNode, GraphNodeKind, GraphNodeSpec, MeshGraph,
};
pub use identity::{accent_for, avatar_seed_for, Accent, ParticipantRef, Principal, PrincipalKind};
pub use meshui::{
    parse_ui_card, ChartKind, KeyValuePair, MeshUiCard, UiAction, UiActionStyle, UiBlock,
};
pub use model::{
    AgentCard, ChatMessage, ConversationSummary, DecisionCard, HomeSnapshot, OptionCard,
    OptionOutcome, TaskCard, UiActionReply,
};
pub use pairing::{parse_pairing_uri, PairingInfo};
pub use presence::Presence;
pub use session::{MeshConfig, MeshSession, SubscriptionHandle};

/// Version of this core library (the workspace version).
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

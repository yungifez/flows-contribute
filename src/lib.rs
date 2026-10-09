//! The Flows contributor client. It labels code with the contributor's own coding agent and
//! sends the labels to the Flows work queue.

pub mod agent;
pub mod api;
pub mod commands;
pub mod config;
pub mod github;
pub mod labels;
pub mod snapshot;

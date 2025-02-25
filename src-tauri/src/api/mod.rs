pub mod admin;
pub mod configuration;
pub mod consumer;
mod error;
mod notification;
pub mod schema_registry;
mod state;
mod types;

// pub use notification::notify_error;
pub use state::AppState;

//! Async Rust client for the `CreateOS` Sandbox API.
//!
//! Start with [`Client::builder`] and [`CreateSandboxRequest`].

#![deny(missing_docs)]

mod client;
mod error;
mod instance;
mod models;
mod services;
mod transport;

pub use client::{Client, ClientBuilder};
pub use error::{ApiError, Error, Result};
pub use instance::{Instance, self_delete, self_pause};
pub use models::*;
pub use services::{
    CommandStream, ComputerService, DisksService, FilesService, KeyboardService, MouseService,
    NetworksService, ProcessStream, ProcessesService, ScreensService, TemplateLogStream,
    TemplatesService, WindowsService,
};

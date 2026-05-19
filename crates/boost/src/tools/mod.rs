//! Built-in MCP tools.

use std::sync::Arc;

use crate::browser::BrowserManager;
use crate::tool::Tool;

pub mod browser_tools;
pub mod commands;
pub mod components;
pub mod config;
pub mod database;
pub mod docs;
pub mod info;
pub mod logs;
pub mod migrations;
pub mod models;
pub mod routes;

pub fn all() -> Vec<Arc<dyn Tool>> {
    let manager = BrowserManager::new();
    vec![
        Arc::new(routes::ListRoutes),
        Arc::new(migrations::ListMigrations),
        Arc::new(models::ListModels),
        Arc::new(components::ListComponents),
        Arc::new(info::ApplicationInfo),
        Arc::new(config::GetConfig),
        Arc::new(commands::ListAvailableCommands),
        Arc::new(database::DatabaseSchema),
        Arc::new(database::DatabaseQuery),
        Arc::new(logs::ReadLogEntries),
        Arc::new(logs::LastError),
        Arc::new(docs::SearchDocs),
        Arc::new(browser_tools::BrowserScreenshot { manager: manager.clone() }),
        Arc::new(browser_tools::BrowserConsole { manager: manager.clone() }),
        Arc::new(browser_tools::BrowserNetwork { manager: manager.clone() }),
        Arc::new(browser_tools::BrowserEval { manager: manager.clone() }),
        Arc::new(browser_tools::BrowserClick { manager: manager.clone() }),
        Arc::new(browser_tools::BrowserFill { manager: manager.clone() }),
        Arc::new(browser_tools::BrowserType { manager: manager.clone() }),
        Arc::new(browser_tools::BrowserWaitFor { manager }),
    ]
}

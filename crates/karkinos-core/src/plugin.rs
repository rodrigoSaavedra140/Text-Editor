//! Extensible Plugin & Scripting Architecture for Karkinos Core.
//! Allows extensions and community plugins to hook into editor lifecycle events,
//! document mutations, and register custom commands.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::document::Document;

/// Metadata describing a command registered by a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCommand {
    pub id: String,
    pub title: String,
    pub description: String,
}

/// Execution context passed to plugins during event dispatch.
#[derive(Debug, Default)]
pub struct PluginContext {
    pub active_file: Option<PathBuf>,
    pub status_message: Option<String>,
}

impl PluginContext {
    pub fn new() -> Self {
        Self {
            active_file: None,
            status_message: None,
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
    }
}

/// Core interface that all Karkinos plugins implement.
pub trait Plugin: Send {
    /// Unique human-readable name of the plugin.
    fn name(&self) -> &str;

    /// Semantic version of the plugin.
    fn version(&self) -> &str;

    /// List of commands provided by this plugin.
    fn commands(&self) -> Vec<PluginCommand> {
        Vec::new()
    }

    /// Called when the plugin is initialized and registered.
    fn on_load(&mut self, _ctx: &mut PluginContext) {}

    /// Called when a document is opened.
    fn on_document_open(&mut self, _doc: &mut Document, _ctx: &mut PluginContext) {}

    /// Called before or after a document is saved to disk.
    fn on_document_save(&mut self, _doc: &mut Document, _ctx: &mut PluginContext) {}

    /// Called when the document text content changes.
    fn on_text_change(&mut self, _doc: &mut Document, _ctx: &mut PluginContext) {}

    /// Dispatches a command registered by this plugin. Returns true if handled.
    fn on_command(
        &mut self,
        _cmd_id: &str,
        _doc: &mut Document,
        _ctx: &mut PluginContext,
    ) -> bool {
        false
    }
}

/// Central coordinator managing active plugins and routing lifecycle events.
pub struct PluginManager {
    plugins: Vec<Box<dyn Plugin>>,
    command_map: HashMap<String, usize>,
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PluginManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.plugins.iter().map(|p| p.name()).collect();
        f.debug_struct("PluginManager")
            .field("plugins", &names)
            .field("commands_count", &self.command_map.len())
            .finish()
    }
}

impl PluginManager {
    /// Creates an empty `PluginManager`.
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            command_map: HashMap::new(),
        }
    }

    /// Creates a `PluginManager` loaded with standard builtin plugins.
    pub fn with_default_plugins() -> Self {
        let mut manager = Self::new();
        manager.register(Box::new(WordCountPlugin));
        manager.register(Box::new(CaseTransformPlugin));
        manager.register(Box::new(HeaderBannerPlugin));
        manager
    }

    /// Registers a new plugin, initializing it and indexing its commands.
    pub fn register(&mut self, mut plugin: Box<dyn Plugin>) {
        let mut ctx = PluginContext::new();
        plugin.on_load(&mut ctx);

        let plugin_idx = self.plugins.len();
        for cmd in plugin.commands() {
            self.command_map.insert(cmd.id, plugin_idx);
        }
        self.plugins.push(plugin);
    }

    /// Returns the total number of registered plugins.
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    /// Returns true if no plugins are registered.
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// Returns all available commands from registered plugins.
    pub fn available_commands(&self) -> Vec<PluginCommand> {
        let mut commands = Vec::new();
        for plugin in &self.plugins {
            commands.extend(plugin.commands());
        }
        commands
    }

    /// Dispatches `on_document_open` across all active plugins.
    pub fn dispatch_document_open(&mut self, doc: &mut Document, ctx: &mut PluginContext) {
        for plugin in &mut self.plugins {
            plugin.on_document_open(doc, ctx);
        }
    }

    /// Dispatches `on_document_save` across all active plugins.
    pub fn dispatch_document_save(&mut self, doc: &mut Document, ctx: &mut PluginContext) {
        for plugin in &mut self.plugins {
            plugin.on_document_save(doc, ctx);
        }
    }

    /// Dispatches `on_text_change` across all active plugins.
    pub fn dispatch_text_change(&mut self, doc: &mut Document, ctx: &mut PluginContext) {
        for plugin in &mut self.plugins {
            plugin.on_text_change(doc, ctx);
        }
    }

    /// Executes a registered command by its ID. Returns true if executed successfully.
    pub fn execute_command(
        &mut self,
        cmd_id: &str,
        doc: &mut Document,
        ctx: &mut PluginContext,
    ) -> bool {
        if let Some(&plugin_idx) = self.command_map.get(cmd_id) {
            if let Some(plugin) = self.plugins.get_mut(plugin_idx) {
                return plugin.on_command(cmd_id, doc, ctx);
            }
        }
        false
    }
}

// -----------------------------------------------------------------------------
// Standard Built-in Plugins
// -----------------------------------------------------------------------------

/// Plugin that computes word, line, and character metrics.
pub struct WordCountPlugin;

impl Plugin for WordCountPlugin {
    fn name(&self) -> &str {
        "Word Counter"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn commands(&self) -> Vec<PluginCommand> {
        vec![PluginCommand {
            id: "word_count".to_string(),
            title: "Calculate Document Word Count".to_string(),
            description: "Computes total lines, words, and characters in the active document".to_string(),
        }]
    }

    fn on_command(&mut self, cmd_id: &str, doc: &mut Document, ctx: &mut PluginContext) -> bool {
        if cmd_id == "word_count" {
            let text = doc.buffer.to_string();
            let lines = doc.buffer.len_lines();
            let words = text.split_whitespace().count();
            let chars = doc.buffer.len_chars();
            ctx.set_status(format!("Document stats: {} lines, {} words, {} chars", lines, words, chars));
            return true;
        }
        false
    }
}

/// Plugin that provides case transformation commands (UPPERCASE, lowercase, Title Case).
pub struct CaseTransformPlugin;

impl Plugin for CaseTransformPlugin {
    fn name(&self) -> &str {
        "Case Transformer"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn commands(&self) -> Vec<PluginCommand> {
        vec![
            PluginCommand {
                id: "uppercase_document".to_string(),
                title: "Convert Document to UPPERCASE".to_string(),
                description: "Transforms the entire document to uppercase".to_string(),
            },
            PluginCommand {
                id: "lowercase_document".to_string(),
                title: "Convert Document to lowercase".to_string(),
                description: "Transforms the entire document to lowercase".to_string(),
            },
        ]
    }

    fn on_command(&mut self, cmd_id: &str, doc: &mut Document, ctx: &mut PluginContext) -> bool {
        match cmd_id {
            "uppercase_document" => {
                let text = doc.buffer.to_string().to_uppercase();
                doc.set_text(&text);
                ctx.set_status("Document converted to UPPERCASE");
                true
            }
            "lowercase_document" => {
                let text = doc.buffer.to_string().to_lowercase();
                doc.set_text(&text);
                ctx.set_status("Document converted to lowercase");
                true
            }
            _ => false,
        }
    }
}

/// Plugin that prepends an authoritative code header banner.
pub struct HeaderBannerPlugin;

impl Plugin for HeaderBannerPlugin {
    fn name(&self) -> &str {
        "Header Banner"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn commands(&self) -> Vec<PluginCommand> {
        vec![PluginCommand {
            id: "insert_header_banner".to_string(),
            title: "Insert File Header Banner".to_string(),
            description: "Prepends a standard source header banner at the top of the file".to_string(),
        }]
    }

    fn on_command(&mut self, cmd_id: &str, doc: &mut Document, ctx: &mut PluginContext) -> bool {
        if cmd_id == "insert_header_banner" {
            let banner = "// ============================================================================\n\
                          // File: Karkinos Source Document\n\
                          // ============================================================================\n\n";
            let mut text = banner.to_string();
            text.push_str(&doc.buffer.to_string());
            doc.set_text(&text);
            ctx.set_status("Header banner inserted");
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_registration_and_available_commands() {
        let manager = PluginManager::with_default_plugins();
        assert_eq!(manager.len(), 3);
        assert!(!manager.is_empty());

        let cmds = manager.available_commands();
        assert!(cmds.iter().any(|c| c.id == "word_count"));
        assert!(cmds.iter().any(|c| c.id == "uppercase_document"));
        assert!(cmds.iter().any(|c| c.id == "insert_header_banner"));
    }

    #[test]
    fn test_execute_word_count_plugin() {
        let mut manager = PluginManager::with_default_plugins();
        let mut doc = Document::new(1);
        doc.set_text("Hello Karkinos world");

        let mut ctx = PluginContext::new();
        let success = manager.execute_command("word_count", &mut doc, &mut ctx);
        assert!(success);
        assert!(ctx.status_message.unwrap().contains("3 words"));
    }

    #[test]
    fn test_execute_case_transform_plugin() {
        let mut manager = PluginManager::with_default_plugins();
        let mut doc = Document::new(2);
        doc.set_text("hello world");

        let mut ctx = PluginContext::new();
        let ok = manager.execute_command("uppercase_document", &mut doc, &mut ctx);
        assert!(ok);
        assert_eq!(doc.buffer.to_string(), "HELLO WORLD");

        let ok_lower = manager.execute_command("lowercase_document", &mut doc, &mut ctx);
        assert!(ok_lower);
        assert_eq!(doc.buffer.to_string(), "hello world");
    }

    #[test]
    fn test_execute_header_banner_plugin() {
        let mut manager = PluginManager::with_default_plugins();
        let mut doc = Document::new(3);
        doc.set_text("fn main() {}");

        let mut ctx = PluginContext::new();
        let ok = manager.execute_command("insert_header_banner", &mut doc, &mut ctx);
        assert!(ok);
        assert!(doc.buffer.to_string().starts_with("// ==="));
        assert!(doc.buffer.to_string().ends_with("fn main() {}"));
    }
}

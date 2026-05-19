use std::collections::BTreeSet;

use crate::tools::ToolRegistry;

pub trait RuntimePlugin {
    fn name(&self) -> &str;
    fn register(&self, registry: &mut ToolRegistry) -> anyhow::Result<()>;
}

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub enabled_modules: BTreeSet<String>,
    pub version: String,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            enabled_modules: [
                "orchestration",
                "context",
                "memory",
                "tools",
                "sandbox",
                "guardrails",
                "verification",
                "subagents",
                "prompts",
                "observability",
                "recovery",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            version: "0.1.0".to_string(),
        }
    }
}

#[derive(Default)]
pub struct Runtime {
    pub config: RuntimeConfig,
    pub registry: ToolRegistry,
    pub plugins: Vec<String>,
}

impl Runtime {
    pub fn install<P: RuntimePlugin>(&mut self, plugin: P) -> anyhow::Result<()> {
        plugin.register(&mut self.registry)?;
        self.plugins.push(plugin.name().to_string());
        Ok(())
    }

    pub fn require(&self, module: &str) -> anyhow::Result<()> {
        if !self.config.enabled_modules.contains(module) {
            anyhow::bail!("Runtime module disabled: {module}");
        }
        Ok(())
    }
}

use crate::{
    WindowApp,
    core::{Error, TabId},
};
use serde_json::Value;
use std::collections::BTreeMap;

/// Tokens prevent an async semantic completion from changing a newer view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HookToken {
    pub host_revision: u64,
    pub model_revision: u64,
    pub configuration_epoch: u64,
    pub tab_id: Option<TabId>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum HookResult {
    Route(Option<String>),
    Title(String),
    Filter(bool),
    Theme(BTreeMap<String, Value>),
    Footer(String),
}

impl WindowApp {
    pub fn hook_token(&self, tab_id: Option<TabId>) -> HookToken {
        HookToken {
            host_revision: self.host_revision.unwrap_or(0),
            model_revision: self.model.revision.wrapping_add(self.geometry_revision),
            configuration_epoch: self.configuration_epoch,
            tab_id,
        }
    }
    /// All results publish together, or none when any token is stale or any result invalid.
    pub fn complete_hook_batch(
        &mut self,
        batch: Vec<(HookToken, HookResult)>,
    ) -> Result<bool, Error> {
        if batch.len() > 4096 {
            return Err(Error("Hook batch exceeds supported limit".into()));
        }
        if self.teardown || batch.is_empty() {
            return Ok(false);
        }
        let current = self.hook_token(None);
        if batch.iter().any(|(token, _)| {
            HookToken {
                tab_id: None,
                ..*token
            } != current
        }) {
            return Ok(false);
        }
        let mut candidate = self.model.clone();
        for (token, result) in batch {
            let tab = |hook: &str| {
                token
                    .tab_id
                    .ok_or_else(|| Error(format!("{hook} hook needs tab ID")))
            };
            match result {
                HookResult::Route(space) => candidate.apply_route_hook(tab("Route")?, space)?,
                HookResult::Title(title) => candidate.apply_title_hook(tab("Title")?, title)?,
                HookResult::Filter(visible) => {
                    candidate.apply_filter_hook(tab("Filter")?, visible)?
                }
                HookResult::Footer(footer) => candidate.apply_footer_hook(footer)?,
                HookResult::Theme(values) => candidate.apply_theme_hook(&values)?,
            }
        }
        candidate.revision = self.model.revision.wrapping_add(1);
        self.model = candidate;
        self.ui.invalidate();
        Ok(true)
    }
}

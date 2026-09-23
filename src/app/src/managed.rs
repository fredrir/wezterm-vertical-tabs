use vtabs_core::{Managed, Model, Space, SpaceTemplate};

/// The plugin-owned Lua file: its last known content and whether a UI edit awaits writing.
#[derive(Default)]
pub(crate) struct ManagedFile {
    /// `None` until a configuration names a file; edits then stay in memory.
    baseline: Option<Managed>,
    dirty: bool,
    writing: bool,
}

impl ManagedFile {
    pub fn projection(model: &Model, spaces: &[Space], templates: &[SpaceTemplate]) -> Managed {
        Managed {
            settings: model.managed_settings().clone(),
            spaces: model
                .spaces
                .iter()
                .filter(|space| {
                    space.template.is_none() && !spaces.iter().any(|s| s.id == space.id)
                })
                .map(|space| Space {
                    collapsed: false,
                    ..space.clone()
                })
                .collect(),
            templates: model
                .templates
                .iter()
                .filter(|template| !templates.iter().any(|t| t.id == template.id))
                .cloned()
                .collect(),
        }
    }
    pub fn baseline(&self) -> Option<&Managed> {
        self.baseline.as_ref()
    }
    pub fn touch(&mut self) {
        self.dirty = true;
    }
    /// Local edits not yet on disk win over file content that predates them.
    pub fn pending(&self, projection: &Managed) -> bool {
        self.writing || self.baseline.as_ref().is_some_and(|b| b != projection)
    }
    pub fn loaded(&mut self, projection: Managed) {
        self.baseline = Some(projection);
        self.dirty = false;
    }
    pub fn take(&mut self, projection: impl FnOnce() -> Managed) -> Option<String> {
        if !self.dirty || self.writing {
            return None;
        }
        self.dirty = false;
        let baseline = self.baseline.as_mut()?;
        let projection = projection();
        if *baseline == projection {
            return None;
        }
        let source = projection.to_lua();
        *baseline = projection;
        self.writing = true;
        Some(source)
    }
    pub fn written(&mut self) {
        self.writing = false;
    }
}

//! Swapping one pane of a split tree for another, keeping the layout.
use crate::pane::{Pane, PaneId};
use crate::tab::{Tab, TabInner};
use crate::{Mux, MuxNotification};
use std::sync::Arc;
use wezterm_term::TerminalSize;

impl Tab {
    pub fn pane_size(&self, pane_id: PaneId) -> anyhow::Result<TerminalSize> {
        self.inner.lock().pane_size(pane_id)
    }

    pub fn replace_pane(
        &self,
        pane_id: PaneId,
        pane: Arc<dyn Pane>,
    ) -> anyhow::Result<TerminalSize> {
        self.inner.lock().replace_pane(pane_id, pane)
    }
}

impl TabInner {
    fn pane_size(&mut self, pane_id: PaneId) -> anyhow::Result<TerminalSize> {
        let position = self
            .iter_panes_ignoring_zoom()
            .into_iter()
            .find(|p| p.pane.pane_id() == pane_id)
            .ok_or_else(|| anyhow::anyhow!("pane {pane_id} is no longer in this tab"))?;
        if position.is_zoomed {
            return Ok(self.size);
        }
        Ok(TerminalSize {
            rows: position.height,
            cols: position.width,
            pixel_width: position.pixel_width,
            pixel_height: position.pixel_height,
            dpi: self.size.dpi,
        })
    }

    fn replace_pane(
        &mut self,
        pane_id: PaneId,
        pane: Arc<dyn Pane>,
    ) -> anyhow::Result<TerminalSize> {
        let size = self.pane_size(pane_id)?;
        // The tab may have been resized while the shell was spawning.
        pane.resize(size)?;
        let prior = self.get_active_pane();
        let mut cursor = self.pane.take().unwrap().cursor();
        loop {
            if let Some(leaf) = cursor.leaf_mut() {
                if leaf.pane_id() == pane_id {
                    *leaf = Arc::clone(&pane);
                    self.pane = Some(cursor.tree());
                    break;
                }
            }
            match cursor.preorder_next() {
                Ok(next) => cursor = next,
                Err(cursor) => {
                    self.pane = Some(cursor.tree());
                    anyhow::bail!("pane {pane_id} is no longer in this tab");
                }
            }
        }
        if self.zoomed.as_ref().is_some_and(|p| p.pane_id() == pane_id) {
            pane.set_zoomed(true);
            self.zoomed = Some(pane);
        }
        self.advise_focus_change(prior);
        Mux::get().notify(MuxNotification::TabResized(self.id));
        Ok(size)
    }
}

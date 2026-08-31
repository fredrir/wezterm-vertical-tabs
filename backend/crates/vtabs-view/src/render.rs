//! Port of layout.lua + render.lua behind one entry point; the golden parity test is the gate.

use crate::scene::RenderInput;

/// Renders a scene and serializes it in the two golden formats:
/// (`<scene>.txt` content with its 2-row column ruler, `<scene>.styled.txt` content).
pub fn golden_dumps(input: &RenderInput) -> (String, String) {
    let _ = input;
    todo!("P4a: port layout.lua and render.lua")
}

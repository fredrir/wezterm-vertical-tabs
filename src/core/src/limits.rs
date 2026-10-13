pub const MAX_ID_BYTES: usize = 128;
pub const MAX_NAME_CHARS: usize = 128;
pub const MAX_SPACES: usize = 256;
pub const MAX_FOLDERS: usize = 256;

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_ID_BYTES && !id.chars().any(char::is_control)
}

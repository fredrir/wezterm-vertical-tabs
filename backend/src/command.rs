use serde::Deserialize;

/// One JSON object per stdin line; unknown tags fail to parse and are dropped.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Command {
    Frame {
        data: String,
    },
    Clear,
    Quit,
    Ping {
        #[serde(default)]
        n: Option<u64>,
    },
    Auth {
        token: String,
    },
}

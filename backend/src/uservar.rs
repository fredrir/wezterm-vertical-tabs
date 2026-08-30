use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

pub const DEFAULT_VAR: &str = "vtabs";
pub const ROLE_VAR: &str = "vtabs_role";
pub const ROLE: &str = "sidebar";

pub fn set_user_var(name: &str, value: &str) -> String {
    format!("\x1b]1337;SetUserVar={name}={}\x07", STANDARD.encode(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_osc_1337_byte_exact() {
        assert_eq!(
            set_user_var("vtabs", r#"{"t":"ping"}"#),
            "\x1b]1337;SetUserVar=vtabs=eyJ0IjoicGluZyJ9\x07"
        );
    }

    #[test]
    fn role_var() {
        assert_eq!(
            set_user_var(ROLE_VAR, ROLE),
            "\x1b]1337;SetUserVar=vtabs_role=c2lkZWJhcg==\x07"
        );
    }

    #[test]
    fn pads_short_values() {
        assert_eq!(set_user_var("x", "a"), "\x1b]1337;SetUserVar=x=YQ==\x07");
    }
}

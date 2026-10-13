use crate::domain::ClientDomainConfig;

#[test]
fn mirrored_spawn_rejects_invalid_targets_before_connecting() {
    use crate::domain::ClientDomain;
    use mux::Mux;
    use std::sync::Arc;

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);
    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            Mux::shutdown();
        }
    }
    let _cleanup = Cleanup;

    for (local_pane_layout, expected) in [
        (false, "pane 123 is no longer open"),
        (
            true,
            "spawn beside a pane requires a mirrored client domain",
        ),
    ] {
        let domain = ClientDomain::new(ClientDomainConfig::Unix(config::UnixDomain {
            local_pane_layout,
            ..Default::default()
        }));
        assert_eq!(
            domain
                .validate_spawn_beside_pane(123)
                .unwrap_err()
                .to_string(),
            expected
        );
        // The future repeats preflight, so a pane gone before polling still fails.
        let error = futures::executor::block_on(domain.spawn_beside_pane(
            123,
            wezterm_term::TerminalSize::default(),
            None,
            None,
            "hidden".to_string(),
        ))
        .err()
        .expect("invalid targets must fail before attempting an RPC");
        assert_eq!(error.to_string(), expected);
    }
}

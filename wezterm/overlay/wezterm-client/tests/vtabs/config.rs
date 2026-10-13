use crate::domain::ClientDomainConfig;

#[test]
fn client_domains_spawn_untabbed_panes_only_with_local_layouts() {
    for local_pane_layout in [false, true] {
        let domains = [
            ClientDomainConfig::Unix(config::UnixDomain {
                local_pane_layout,
                ..Default::default()
            }),
            ClientDomainConfig::Tls(config::TlsDomainClient {
                local_pane_layout,
                ..Default::default()
            }),
            ClientDomainConfig::Ssh(config::SshDomain {
                local_pane_layout,
                ..Default::default()
            }),
        ];
        for domain in domains {
            assert_eq!(domain.local_pane_layout(), local_pane_layout);
        }
    }
}

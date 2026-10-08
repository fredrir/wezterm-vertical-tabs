use crate::cli::list::ListCommand;
use clap::Parser;
use codec::ListPanesResponse;
use mux::pane::PaneId;
use mux::tab::PaneNode;
use mux::window::WindowId;
use wezterm_client::client::Client;

#[derive(Debug, Parser, Clone)]
pub struct AdoptPane {
    /// The local_pane_layout client domain that reaches the remote mux.
    /// It is attached first when needed, which restores this host's
    /// orphaned backing tabs into the "__detached" workspace.
    #[arg(long)]
    domain_name: String,

    /// List the remote panes that can be adopted instead of adopting one.
    #[arg(long, conflicts_with_all = ["remote_pane_id", "window_id"])]
    list: bool,

    #[command(flatten)]
    listing: ListCommand,

    /// The remote mux's id of the pane to adopt.
    #[arg(long, required_unless_present = "list")]
    remote_pane_id: Option<PaneId>,

    /// The window that receives the new tab.
    /// If omitted, the tab gets a new window in the "__detached" workspace.
    #[arg(long)]
    window_id: Option<WindowId>,
}

impl AdoptPane {
    pub async fn run(&self, client: Client) -> anyhow::Result<()> {
        let domain = self.domain_name.clone();
        let Some(remote_pane_id) = self.remote_pane_id else {
            let panes = client
                .list_adoptable_panes(codec::ListAdoptablePanes { domain })
                .await?
                .panes;
            return self.listing.print(ListPanesResponse {
                tab_titles: vec![String::new(); panes.len()],
                tabs: panes.into_iter().map(PaneNode::Leaf).collect(),
                window_titles: Default::default(),
            });
        };
        let adopted = client
            .adopt_pane(codec::AdoptPane {
                domain,
                remote_pane_id,
                window_id: self.window_id,
            })
            .await?;
        println!("{}", adopted.pane_id);
        Ok(())
    }
}

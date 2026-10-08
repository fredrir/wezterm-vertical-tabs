use super::*;

#[test]
fn vtabs_attaching_to_an_idle_pane_replays_existing_user_vars_once() {
    config::designate_this_as_the_main_thread();
    let pair = portable_pty::native_pty_system()
        .openpty(portable_pty::PtySize::default())
        .unwrap();
    let child = pair
        .slave
        .spawn_command(portable_pty::CommandBuilder::new("/usr/bin/true"))
        .unwrap();
    let writer = pair.master.take_writer().unwrap();
    let mut terminal = wezterm_term::Terminal::new(
        wezterm_term::TerminalSize::default(),
        Arc::new(TermConfig::new()),
        "test",
        "test",
        Box::new(std::io::sink()),
    );
    // This value was emitted before the client attached.
    terminal.advance_bytes(b"\x1b]1337;SetUserVar=vtabs_jobs=YmVmb3Jl\x07");
    let pane: Arc<dyn Pane> = Arc::new(mux::localpane::LocalPane::new(
        1,
        terminal,
        child,
        pair.master,
        writer,
        0,
        String::new(),
    ));
    let messages = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&messages);
    let sender = PduSender::new(move |message| {
        if let Pdu::NotifyAlert(NotifyAlert {
            pane_id,
            alert: Alert::SetUserVar { name, value },
        }) = message.pdu
        {
            captured.lock().unwrap().push((pane_id, name, value));
        }
        Ok(())
    });
    let connection = Arc::new(Mutex::new(PerPane::default()));
    maybe_push_pane_changes(&pane, sender.clone(), Arc::clone(&connection)).unwrap();
    assert_eq!(
        *messages.lock().unwrap(),
        vec![(1, "vtabs_jobs".into(), "before".into())]
    );
    messages.lock().unwrap().clear();
    maybe_push_pane_changes(&pane, sender.clone(), connection).unwrap();
    assert!(messages.lock().unwrap().is_empty());
    // A different connection receives its own snapshot, including idle panes.
    maybe_push_pane_changes(&pane, sender, Arc::new(Mutex::new(PerPane::default()))).unwrap();
    assert_eq!(
        *messages.lock().unwrap(),
        vec![(1, "vtabs_jobs".into(), "before".into())]
    );
}

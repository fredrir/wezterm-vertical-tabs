use vtabs_core::*;

fn tab(id: TabId) -> Tab {
    Tab {
        id,
        launch: Some(LaunchSpec {
            args: vec![id.to_string()],
            ..LaunchSpec::default()
        }),
        ..Tab::default()
    }
}
fn model() -> Model {
    let mut model = Model::default();
    model
        .reconcile(vec![tab(1), tab(2), tab(3)], Some(1), true)
        .unwrap();
    model
}

#[test]
fn acknowledged_transfer_preserves_unrelated_actual_close_history() {
    let mut model = model();
    model
        .reconcile(vec![tab(2), tab(3)], Some(2), true)
        .unwrap();
    model.acknowledge_tab_departure(2);
    model.reconcile(vec![tab(3)], Some(3), true).unwrap();
    let result = model.dispatch(Intent::Reopen).unwrap();
    assert!(
        matches!(&result.commands[..], [HostCommand::Spawn { launch, .. }] if launch.args == ["1"])
    );
    assert!(!model.can_reopen());
}

#[test]
fn delayed_native_transfer_ack_removes_already_recorded_history() {
    let mut model = model();
    model
        .reconcile(vec![tab(2), tab(3)], Some(2), true)
        .unwrap();
    assert!(model.can_reopen());
    assert!(model.acknowledge_tab_departure(1));
    assert!(!model.can_reopen());
}

#[test]
fn failed_unacknowledged_move_does_not_suppress_a_later_actual_close() {
    let mut model = model();
    model.dispatch(Intent::MoveTabToNewWindow(1)).unwrap();
    // A rejected host move leaves the source topology intact and sends no departure.
    model
        .reconcile(vec![tab(1), tab(2), tab(3)], Some(1), false)
        .unwrap();
    model
        .reconcile(vec![tab(2), tab(3)], Some(2), true)
        .unwrap();
    let result = model.dispatch(Intent::Reopen).unwrap();
    assert!(
        matches!(&result.commands[..], [HostCommand::Spawn { launch, .. }] if launch.args == ["1"])
    );
}

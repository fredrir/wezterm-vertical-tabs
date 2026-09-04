use super::*;

#[test]
fn command_output_is_drained_before_the_child_exits() {
    let mut command = Command::new("sh");
    command.args([
        "-c",
        "dd if=/dev/zero bs=65536 count=4 2>/dev/null; dd if=/dev/zero bs=65536 count=4 1>&2 2>/dev/null",
    ]);
    let out = run_command(&mut command, Duration::from_secs(3), "bulk output", None).unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout.len(), 4 * 65_536, "larger than a pipe buffer");
    assert_eq!(out.stderr.len(), 4 * 65_536, "both pipes are drained");
}

#[test]
fn command_output_capture_is_bounded_while_both_pipes_are_drained() {
    let mut command = Command::new("sh");
    command.args([
        "-c",
        "dd if=/dev/zero bs=65536 count=17 2>/dev/null; dd if=/dev/zero bs=65536 count=17 1>&2 2>/dev/null",
    ]);
    assert_eq!(
        run_command(&mut command, Duration::from_secs(3), "excess output", None).unwrap_err(),
        format!("excess output: output exceeded {OUTPUT_MAX} bytes")
    );
}

#[test]
fn a_timed_out_command_is_killed_and_reaped() {
    let mut command = Command::new("sh");
    // The one-second descendant inherits both pipes after the five-second direct child dies.
    command.args(["-c", "sleep 1 & exec sleep 5"]);
    let started = Instant::now();
    assert_eq!(
        run_command(
            &mut command,
            Duration::from_millis(30),
            "slow command",
            None
        )
        .unwrap_err(),
        "slow command timed out"
    );
    assert!(started.elapsed() < Duration::from_millis(750));
}

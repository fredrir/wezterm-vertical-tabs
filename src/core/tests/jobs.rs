use super::*;

const SNAPSHOT: &str =
    "1\t123\t4\t1\tremote\n1\t456\trunning\tsleep 60\n2\t789\tsuspended\tcat | cat";

#[test]
fn jobs_keep_shell_identity_and_pipeline_command() {
    let shell = ShellJobs::parse(SNAPSHOT).unwrap();
    assert_eq!(shell.host, "remote");
    assert_eq!(shell.jobs.len(), 2);
    assert!(shell.jobs[1].suspended);
    assert_eq!(shell.jobs[1].command, "cat | cat");
    let target = JobTarget {
        pane: 9,
        shell: 123,
        number: 2,
        pid: 789,
    };
    assert_eq!(
        shell.request(target, JobOperation::Foreground).unwrap(),
        "\x1b[777;123;4;2;789;1~"
    );
    assert_eq!(
        shell.request(target, JobOperation::Background).unwrap(),
        "\x1b[777;123;4;2;789;2~"
    );
    assert_eq!(
        shell.request(target, JobOperation::Terminate).unwrap(),
        "\x1b[777;123;4;2;789;3~"
    );
}

#[test]
fn stale_rows_and_busy_shells_cannot_receive_actions() {
    let mut shell = ShellJobs::parse(SNAPSHOT).unwrap();
    let target = JobTarget {
        pane: 9,
        shell: 123,
        number: 1,
        pid: 456,
    };
    for stale in [
        JobTarget {
            shell: 124,
            ..target
        },
        JobTarget { pid: 999, ..target },
        JobTarget {
            number: 3,
            ..target
        },
    ] {
        assert_eq!(
            shell.request(stale, JobOperation::Terminate),
            Err("Job no longer exists")
        );
    }
    shell.ready = false;
    assert!(shell.refresh().is_none());
    assert!(shell.request(target, JobOperation::Foreground).is_err());
}

#[test]
fn malformed_or_unsupported_snapshots_are_ignored() {
    for value in [
        "",
        "2\t123\t4\t1\thost",
        "1\t0\t4\t1\thost",
        "1\t123\t4\tmaybe\thost",
        "1\t123\t4\t1\thost\textra",
        "1\t123\t4\t1\thost\n1\t0\trunning\tx",
        "1\t123\t4\t1\thost\n1\t10\tdone\tx",
        "1\t123\t4\t1\thost\n1\t10\trunning\tx\tx",
        "1\t123\t4\t1\thost\n1\t10\trunning\tx\n1\t11\trunning\ty",
        "1\t123\t4\t1\thost\n1\t10\trunning\tx\x1b[0m",
    ] {
        assert!(ShellJobs::parse(value).is_none(), "{value:?}");
    }
    assert!(ShellJobs::parse(&"x".repeat(256 * 1024 + 1)).is_none());
    assert!(
        ShellJobs::parse("1\t123\t4\t1\thost")
            .unwrap()
            .jobs
            .is_empty()
    );
}

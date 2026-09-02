use vtabs_runtime::log::Logger;

fn main() {
    // `frame` renders the Zen background and exits; it never touches the terminal or the protocol.
    let mut args = std::env::args().skip(1);
    if let Some("frame") = args.next().as_deref() {
        if let Err(err) = vtabs_zen::frame::run(args) {
            eprintln!("{err}");
            std::process::exit(2);
        }
        return;
    }
    if let Err(err) = vtabs_runtime::run() {
        Logger::from_env().log(format!("exit: {err}"));
        std::process::exit(1);
    }
}

fn main() {
    println!(
        "cargo:rustc-env=WEZ_VTABS_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
}

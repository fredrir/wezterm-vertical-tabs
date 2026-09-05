# WezTerm vertical tabs

Native vertical tabs with rounded window framing, folders, spaces and a dedicated settings page, rendered by WezTerm.

| Name | Value |
| --- | --- |
| Build | Latest WezTerm `main` plus a compact native patch series |
| Platforms | macOS, Linux, Windows |
| Sessions | Local, Unix mux where supported, SSH mux, mutually authenticated TLS mux |
| State | Project-owned SQLite helper on the GUI machine |
| Configuration | Rust defaults; optional Lua styling and semantic hooks |
| Navigation | Native WezTerm tab actions follow the selected space's visible tabs |

```sh
git clone --branch native https://github.com/fredrir/wezterm-vertical-tabs.git
cd wezterm-vertical-tabs
python3 scripts/native.py install
python3 scripts/native.py launch
```

On Windows, use `py -3` in place of `python3`. Requires Git, stable Rust, Python 3.10+, and the platform dependencies for [building WezTerm](https://wezterm.org/install/source.html). The managed launch entry builds updates asynchronously and selects completed bundles between launches.

| Reference | Value |
| --- | --- |
| [Configuration](docs/configuration.md) | Lua options, spaces, themes and hooks |
| [Development](docs/development.md) | Build, packaging, updates and native GUI checks |
| [Validation](docs/validation.md) | Integrated measurements, regression checks and verification limits |
| [Boundaries](docs/limitations.md) | Persistence, remote operations and platform requirements |

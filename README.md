# wezterm-vertical-tabs (WiP)


## Installation

```sh
local wezterm = require "wezterm"
local config = wezterm.config_builder()

-- your own bindings first: the plugin never overrides a key you already bound
config.keys = { ... }

local vtabs = wezterm.plugin.require "https://github.com/fredrir/wezterm-vertical-tabs"
vtabs.apply_to_config(config, {
  width = 28,
})
return config
```

## Configuration

See [configuration.md](docs/configuration) for configuration options

# wezterm-vertical-tabs (WiP)


## Installation

```sh
local vtabs = wezterm.plugin.require "https://github.com/fredrir/wezterm-vertical-tabs"
vtabs.apply_to_config(config, {
  width = 28,
  spaces = {
    { id = "home", icon = "󰋜" },
    { id = "claude", icon = "", theme = { accent = "#f5c2e7" }, match = { proc = "claude" } },
    { id = "$host", icon = "󰒋", match = { remote = true }, theme = "auto" },
  },
})
```
> Apply the plugin after your own key bindings so its defaults can be overridden or disabled per binding. 


## Configuration

See [configuration.md](docs/configuration) for configuration options

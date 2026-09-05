use crate::scripting::guiwin::GuiWin;
use crate::termwindow::TermWindowNotif;
use config::lua::{get_or_create_sub_module, mlua};
use mlua::{Lua, LuaSerdeExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{LazyLock, Mutex},
    time::Duration,
};
use vtabs_app::{core, HookResult, HookToken, WindowApp};
use window::{Window, WindowOps};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Configuration {
    #[serde(default = "default_profile")]
    pub profile: String,
    pub settings: BTreeMap<String, Value>,
    pub spaces: Option<Vec<core::Space>>,
    pub templates: Vec<core::SpaceTemplate>,
}
fn default_profile() -> String {
    "default".into()
}
impl Configuration {
    pub fn value(&self) -> Value {
        let mut value = json!({"profile": self.profile, "settings": self.settings, "templates": self.templates});
        if let Some(spaces) = &self.spaces {
            value["spaces"] = json!(spaces);
        }
        value
    }
}
const CONFIG_REGISTRY: &str = "wez-vtabs-configuration";
const MAX_HOOK_RESULTS: usize = 4096;
const MAX_RESULT_BYTES: usize = 64 * 1024;
const MAX_BATCH_BYTES: usize = 1024 * 1024;
const HOOK_TIMEOUT: Duration = Duration::from_secs(2);
const TAB_HOOKS: [&str; 3] = ["title", "routing", "filter"];
const WINDOW_HOOKS: [&str; 2] = ["theme", "footer"];
/// Leave room for one theme and footer result. The scheduler retains overflow tabs.
pub const MAX_HOOK_TABS: usize = (MAX_HOOK_RESULTS - WINDOW_HOOKS.len()) / TAB_HOOKS.len();

impl Default for Configuration {
    fn default() -> Self {
        Self {
            profile: default_profile(),
            settings: BTreeMap::new(),
            spaces: None,
            templates: Vec::new(),
        }
    }
}

#[derive(Default)]
struct Warnings {
    generation: usize,
    kinds: BTreeSet<&'static str>,
}
static WARNINGS: LazyLock<Mutex<Warnings>> = LazyLock::new(|| Mutex::new(Warnings::default()));
fn warn_once(kind: &'static str, error: impl std::fmt::Display) {
    let generation = config::configuration().generation();
    let fresh = {
        let mut warnings = WARNINGS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if warnings.generation != generation {
            warnings.generation = generation;
            warnings.kinds.clear();
        }
        warnings.kinds.insert(kind)
    };
    if fresh {
        log::warn!("native tabs {kind}: {error}");
    }
}

fn configuration_from_lua(lua: &Lua) -> anyhow::Result<Configuration> {
    let value = lua.named_registry_value::<mlua::Value>(CONFIG_REGISTRY)?;
    if matches!(value, mlua::Value::Nil) {
        return Ok(Configuration::default());
    }
    Ok(lua.from_value(value)?)
}

/// Only configuration/new-window paths call this. Reading the current published Lua
/// state means a failed reload cannot replace the last valid native configuration.
pub fn configuration() -> Configuration {
    match config::run_immediate_with_lua_config(|lua| match lua {
        Some(lua) => configuration_from_lua(&lua),
        None => Ok(Configuration::default()),
    }) {
        Ok(configuration) => configuration,
        Err(error) => {
            warn_once("configuration", error);
            Configuration::default()
        }
    }
}

/// Cache this at a configuration epoch boundary, never on native activation or paint.
pub fn hooks_enabled() -> bool {
    let result = config::run_immediate_with_lua_config(|lua| {
        let Some(lua) = lua else { return Ok(false) };
        let module = get_or_create_sub_module(&lua, "native_tabs")?;
        let value = module.raw_get::<_, mlua::Value>("hooks")?;
        let callbacks = match value {
            mlua::Value::Nil => return Ok(false),
            mlua::Value::Table(callbacks) => callbacks,
            _ => anyhow::bail!("hooks must be a table of functions"),
        };
        for name in TAB_HOOKS.iter().chain(WINDOW_HOOKS.iter()).copied() {
            if matches!(
                callbacks.raw_get::<_, mlua::Value>(name)?,
                mlua::Value::Function(_)
            ) {
                return Ok(true);
            }
        }
        Ok(false)
    });
    match result {
        Ok(enabled) => enabled,
        Err(error) => {
            warn_once("hooks", error);
            false
        }
    }
}

pub struct WindowHookContext {
    token: HookToken,
    value: Value,
}
impl WindowHookContext {
    pub fn from_app(app: &WindowApp) -> Self {
        let model = app.model();
        Self {
            token: app.hook_token(None),
            value: json!({
                "profile": model.profile, "private": model.private,
                "selected_space": model.selected_space, "space": model.selected_space(),
                "active_tab": model.selected_tab, "settings": model.settings,
            }),
        }
    }
}

pub fn register(lua: &Lua) -> anyhow::Result<()> {
    let module = get_or_create_sub_module(lua, "native_tabs")?;
    module.set("capability", 1)?;
    module.set("schema", lua.to_value(&core::Settings::schema())?)?;
    module.set(
        "configure",
        lua.create_function(|lua, value: mlua::Value| {
            let config: Configuration = lua.from_value(value)?;
            let mut app = WindowApp::new(&config.profile, false);
            app.config(config.value()).map_err(mlua::Error::external)?;
            // This state is published only after WezTerm accepts the entire Lua config.
            lua.set_named_registry_value(CONFIG_REGISTRY, lua.to_value(&config)?)?;
            Ok(())
        })?,
    )?;
    module.set(
        "dispatch",
        lua.create_function(
            |lua, (window, action): (mlua::UserDataRef<GuiWin>, mlua::Value)| {
                let action: Value = lua.from_value(action)?;
                let mux_window_id = window.mux_window_id;
                window
                    .window
                    .notify(TermWindowNotif::Apply(Box::new(move |tw| {
                        tw.native_message_for(mux_window_id, json!({"action":action}))
                    })));
                Ok(())
            },
        )?,
    )?;
    module.set("inspect", lua.create_async_function(|lua, window: mlua::UserDataRef<GuiWin>| async move {
        let (tx,rx) = smol::channel::bounded(1);
        window.window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
            tw.native_sync();
            let geometry = tw.native_geometry();
            let value = if let Some(ui) = tw.native_ui.as_ref() {
                json!({"visible":ui.projection.tabs,"active":ui.projection.active,
                    "sidebar":{"x":geometry.sidebar.x,"y":geometry.sidebar.y,"width":geometry.sidebar.width,"height":geometry.sidebar.height},
                    "content":{"x":geometry.content.x,"y":geometry.content.y,"width":geometry.content.width,"height":geometry.content.height},
                    "model":ui.provider.inspect(),"paint":ui.stats.inspect(),"frame_cpu_us":tw.native_last_frame_us()})
            } else { Value::Null };
            tx.try_send(value).ok();
        })));
        let value = rx.recv().await.map_err(mlua::Error::external)?;
        lua.to_value(&value)
    })?)?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HookCompletion {
    token: HookToken,
    result: HookResult,
}

fn decode_result(name: &str, value: Value) -> anyhow::Result<Option<HookResult>> {
    let result = match name {
        "title" if value.is_null() => None,
        "title" => Some(HookResult::Title(
            value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("title must return a string or nil"))?
                .into(),
        )),
        "routing" if value.is_null() => Some(HookResult::Route(None)),
        "routing" => Some(HookResult::Route(Some(
            value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("routing must return a space ID or nil"))?
                .into(),
        ))),
        "filter" if value.is_null() => None,
        "filter" => {
            Some(HookResult::Filter(value.as_bool().ok_or_else(|| {
                anyhow::anyhow!("filter must return a boolean or nil")
            })?))
        }
        "theme" if value.is_null() => None,
        "theme" => Some(HookResult::Theme(serde_json::from_value(value)?)),
        "footer" if value.is_null() => Some(HookResult::Footer(String::new())),
        "footer" => {
            let footer = match value {
                Value::String(text) => text,
                Value::Array(rows) if rows.len() <= 16 => rows
                    .into_iter()
                    .map(|row| {
                        row.as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| anyhow::anyhow!("footer rows must be strings"))
                    })
                    .collect::<anyhow::Result<Vec<_>>>()?
                    .join("\n"),
                _ => anyhow::bail!("footer must return a string, up to 16 text rows, or nil"),
            };
            Some(HookResult::Footer(footer))
        }
        _ => anyhow::bail!("unknown hook"),
    };
    if let Some(result) = &result {
        anyhow::ensure!(
            serde_json::to_vec(result)?.len() <= MAX_RESULT_BYTES,
            "hook result exceeds 64 KiB"
        );
    }
    Ok(result)
}

async fn call_hook<'lua>(
    lua: &'lua Lua,
    callbacks: &mlua::Table<'lua>,
    name: &'static str,
    input: mlua::Value<'lua>,
) -> Option<HookResult> {
    let function = match callbacks.raw_get::<_, mlua::Value>(name) {
        Ok(mlua::Value::Function(function)) => function,
        Ok(mlua::Value::Nil) => return None,
        Ok(_) => {
            warn_once(name, "hook must be a function");
            return None;
        }
        Err(error) => {
            warn_once(name, error);
            return None;
        }
    };
    match function.call_async::<_, mlua::Value>(input).await {
        Ok(value) => match lua
            .from_value::<Value>(value)
            .map_err(anyhow::Error::from)
            .and_then(|value| decode_result(name, value))
        {
            Ok(result) => result,
            Err(error) => {
                warn_once(name, error);
                None
            }
        },
        Err(error) => {
            warn_once(name, error);
            None
        }
    }
}

pub fn hooks(
    window: Window,
    batch: Vec<(HookToken, core::Tab)>,
    context: WindowHookContext,
    mux_window_id: usize,
) {
    // Completion belongs to the outer future: serialization, registry errors and timeout
    // all clear the adapter's outstanding flag instead of permanently wedging its queue.
    promise::spawn::spawn(async move {
        let operation = config::with_lua_config_on_main_thread(move |lua| async move {
            let Some(lua) = lua else {
                return Ok(Vec::new());
            };
            let module = get_or_create_sub_module(&lua, "native_tabs")?;
            let callbacks = match module.raw_get::<_, mlua::Value>("hooks")? {
                mlua::Value::Nil => return Ok(Vec::new()),
                mlua::Value::Table(callbacks) => callbacks,
                _ => anyhow::bail!("hooks must be a table"),
            };
            if batch.len() > MAX_HOOK_TABS {
                warn_once("hook capacity", "scheduler exceeded the tab batch limit");
            }
            let mut results = Vec::with_capacity(
                batch.len().min(MAX_HOOK_TABS) * TAB_HOOKS.len() + WINDOW_HOOKS.len(),
            );
            let mut bytes = 0;
            for (index, (token, tab)) in batch.into_iter().take(MAX_HOOK_TABS).enumerate() {
                let input = lua.to_value(&tab)?;
                for name in TAB_HOOKS {
                    if let Some(result) = call_hook(&lua, &callbacks, name, input.clone()).await {
                        let completion = HookCompletion { token, result };
                        bytes += serde_json::to_vec(&completion)?.len();
                        anyhow::ensure!(bytes <= MAX_BATCH_BYTES, "hook batch exceeds 1 MiB");
                        results.push(completion);
                    }
                }
                if index % 16 == 15 {
                    smol::future::yield_now().await;
                }
            }
            let input = lua.to_value(&context.value)?;
            for name in WINDOW_HOOKS {
                if let Some(result) = call_hook(&lua, &callbacks, name, input.clone()).await {
                    let completion = HookCompletion {
                        token: context.token,
                        result,
                    };
                    bytes += serde_json::to_vec(&completion)?.len();
                    anyhow::ensure!(bytes <= MAX_BATCH_BYTES, "hook batch exceeds 1 MiB");
                    results.push(completion);
                }
            }
            Ok(results)
        });
        let results = match smol::future::race(operation, async {
            smol::Timer::after(HOOK_TIMEOUT).await;
            anyhow::bail!("hook batch exceeded two seconds")
        })
        .await
        {
            Ok(results) => results,
            Err(error) => {
                warn_once("hook batch", error);
                Vec::new()
            }
        };
        window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
            tw.native_message_for(mux_window_id, json!({"hooks":results}))
        })));
    })
    .detach();
}

pub fn complete(app: &mut WindowApp, message: Value) -> bool {
    let Some(hooks) = message.get("hooks").and_then(Value::as_array) else {
        return true;
    };
    if hooks.is_empty() {
        return true;
    }
    if hooks.len() > MAX_HOOK_RESULTS {
        warn_once("hook result", "result count exceeds current contract");
        return true;
    }
    let results = match serde_json::from_value::<Vec<HookCompletion>>(Value::Array(hooks.clone())) {
        Ok(results) => results,
        Err(error) => {
            warn_once("hook result", error);
            return true;
        }
    };
    match app.complete_hook_batch(
        results
            .into_iter()
            .map(|item| (item.token, item.result))
            .collect(),
    ) {
        Ok(applied) => applied,
        Err(error) => {
            warn_once("hook result", error);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_configuration_survives_without_declared_spaces() {
        let mut configuration = Configuration::default();
        configuration.templates.push(core::SpaceTemplate {
            id: "host-$host".into(),
            name: "$host".into(),
            icon: "".into(),
            accent: None,
            rules: Vec::new(),
        });
        let value = configuration.value();
        assert_eq!(value["profile"], "default");
        assert!(value.get("spaces").is_none());
        assert_eq!(value["templates"].as_array().unwrap().len(), 1);
    }
    #[test]
    fn routing_type_errors_cannot_become_route_reset() {
        assert!(decode_result("routing", json!(false)).is_err());
        assert!(matches!(
            decode_result("routing", Value::Null).unwrap(),
            Some(HookResult::Route(None))
        ));
    }
    #[test]
    fn footer_rows_are_bounded_and_normalized() {
        assert!(
            matches!(decode_result("footer", json!(["One", "Two"])).unwrap(), Some(HookResult::Footer(text)) if text == "One\nTwo")
        );
        assert!(decode_result("footer", json!(["One", 2])).is_err());
        assert!(decode_result("footer", json!(vec!["row"; 17])).is_err());
    }
    #[test]
    fn batch_capacity_includes_window_hooks() {
        assert!(MAX_HOOK_TABS * TAB_HOOKS.len() + WINDOW_HOOKS.len() <= MAX_HOOK_RESULTS);
        assert!((MAX_HOOK_TABS + 1) * TAB_HOOKS.len() + WINDOW_HOOKS.len() > MAX_HOOK_RESULTS);
    }
    #[test]
    fn each_lua_configuration_keeps_its_own_validated_value() {
        let a = Lua::new();
        let b = Lua::new();
        let configuration = Configuration {
            profile: "kept".into(),
            ..Configuration::default()
        };
        a.set_named_registry_value(CONFIG_REGISTRY, a.to_value(&configuration).unwrap())
            .unwrap();
        assert_eq!(configuration_from_lua(&a).unwrap().profile, "kept");
        assert_eq!(configuration_from_lua(&b).unwrap().profile, "default");
    }
}

//! Keeps the files in `examples/` valid: every plugin config block parses without
//! warnings, and the Claude settings only register known hook events.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use claude_sessions_core::config::{Config, Mode};
use claude_sessions_core::hook_map::event_to_state;
use kdl::{KdlDocument, KdlNode, KdlValue};

/// Options of the `LaunchOrFocusPlugin` action itself, not passed to the plugin.
const ACTION_OPTIONS: &[&str] = &[
    "floating",
    "move_to_focused_tab",
    "in_place",
    "skip_plugin_cache",
];

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn value_to_string(value: &KdlValue) -> String {
    match value {
        KdlValue::String(s) | KdlValue::RawString(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Config blocks of every `plugin` node or `LaunchOrFocusPlugin` action that loads this plugin.
fn plugin_configs(doc: &KdlDocument, found: &mut Vec<BTreeMap<String, String>>) {
    for node in doc.nodes() {
        if is_our_plugin(node) {
            let config = node
                .children()
                .map(|children| {
                    children
                        .nodes()
                        .iter()
                        .filter(|c| !ACTION_OPTIONS.contains(&c.name().value()))
                        .map(|c| {
                            let value = c.entries().first().map(|e| value_to_string(e.value()));
                            (c.name().value().to_string(), value.unwrap_or_default())
                        })
                        .collect()
                })
                .unwrap_or_default();
            found.push(config);
        } else if let Some(children) = node.children() {
            plugin_configs(children, found);
        }
    }
}

fn is_our_plugin(node: &KdlNode) -> bool {
    let location = match node.name().value() {
        "plugin" => node.get("location"),
        "LaunchOrFocusPlugin" => node.entries().iter().find(|e| e.name().is_none()),
        _ => None,
    };
    location
        .and_then(|e| e.value().as_string())
        .is_some_and(|l| l.contains("claude-sessions"))
}

#[test]
fn kdl_examples_parse_and_plugin_config_is_valid() {
    let mut modes = Vec::new();
    for entry in fs::read_dir(examples_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|x| x != "kdl") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        let doc: KdlDocument = text
            .parse()
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));

        let mut configs = Vec::new();
        plugin_configs(&doc, &mut configs);
        assert!(
            !configs.is_empty(),
            "{}: no plugin block found",
            path.display()
        );

        for raw in configs {
            let (config, warnings) = Config::parse(&raw);
            assert!(warnings.is_empty(), "{}: {warnings:?}", path.display());
            modes.push(config.mode);
        }
    }
    assert!(modes.contains(&Mode::Picker), "no picker example");
    assert!(modes.contains(&Mode::Bar), "no bar example");
}

#[test]
fn claude_settings_example_registers_known_events() {
    let text = fs::read_to_string(examples_dir().join("claude-settings.json")).unwrap();
    let settings: serde_json::Value = serde_json::from_str(&text).unwrap();
    let hooks = settings["hooks"].as_object().expect("hooks object");

    for (event, groups) in hooks {
        assert!(
            event_to_state(event, None).is_some(),
            "unmapped hook event {event}"
        );
        for group in groups.as_array().unwrap() {
            for hook in group["hooks"].as_array().unwrap() {
                let command = hook["command"].as_str().unwrap();
                assert!(
                    command.contains("claude-zellij-hook.sh"),
                    "{event}: {command}"
                );
            }
        }
    }
    for required in ["SessionStart", "UserPromptSubmit", "Stop", "SessionEnd"] {
        assert!(hooks.contains_key(required), "missing {required}");
    }
}

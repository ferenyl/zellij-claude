//! Which floating panes the picker hides while open, so only the picker shows up when
//! the user's floating layer was hidden.

use std::collections::BTreeMap;

/// Floating layer visibility per tab position, recorded by the bar for the picker:
/// Zellij sends no `TabUpdate` to the hidden picker.
pub type LayerStates = BTreeMap<usize, bool>;

/// Position and size of a pane, including its frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub x: usize,
    pub y: usize,
    pub cols: usize,
    pub rows: usize,
}

/// A pane of the picker's tab, converted by the plugin shell from `PaneUpdate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneSnapshot {
    pub id: u32,
    pub is_plugin: bool,
    pub is_floating: bool,
    pub is_suppressed: bool,
    pub plugin_url: Option<String>,
    pub geometry: Geometry,
}

/// A floating pane the picker hid, with the geometry to restore it at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiddenPane {
    pub id: u32,
    pub is_plugin: bool,
    pub geometry: Geometry,
}

/// Panes hidden while the picker is open, and the tab whose floating layer to hide again
/// (`None` = the client's active tab).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Isolation {
    pub tab_id: Option<usize>,
    pub hidden: Vec<HiddenPane>,
}

/// Visible floating panes other than the picker, or none if the layer was already
/// visible before the picker opened (they were on screen anyway).
pub fn panes_to_isolate(
    own_plugin_id: u32,
    panes: &[PaneSnapshot],
    layer_was_visible: bool,
) -> Vec<HiddenPane> {
    if layer_was_visible {
        return Vec::new();
    }
    panes
        .iter()
        .filter(|p| p.is_floating && !p.is_suppressed)
        .filter(|p| !(p.is_plugin && p.id == own_plugin_id))
        .map(|p| HiddenPane {
            id: p.id,
            is_plugin: p.is_plugin,
            geometry: p.geometry,
        })
        .collect()
}

/// Whether the picker's own pane went from hidden (or unknown) to shown.
/// Arguments are its `is_suppressed` flag before and now; `None` = not found.
pub fn became_visible(was_suppressed: Option<bool>, is_suppressed: Option<bool>) -> bool {
    is_suppressed == Some(false) && was_suppressed != Some(false)
}

/// Whether the shown picker was moved to another tab (`move_to_focused_tab`) without
/// being hidden first. Arguments are its tab position and `is_suppressed` flag.
pub fn moved_tab(was: Option<(usize, bool)>, now: Option<(usize, bool)>) -> bool {
    matches!((was, now), (Some((from, false)), Some((to, false))) if from != to)
}

/// Tab position of a shown picker: a floating plugin pane loaded from the same URL as
/// this instance. `panes` pairs each pane with its tab position.
pub fn shown_picker_position(own_plugin_id: u32, panes: &[(usize, PaneSnapshot)]) -> Option<usize> {
    let own_url = panes
        .iter()
        .find(|(_, p)| p.is_plugin && p.id == own_plugin_id)
        .and_then(|(_, p)| p.plugin_url.as_ref())?;
    panes
        .iter()
        .find(|(_, p)| {
            p.is_plugin
                && p.id != own_plugin_id
                && p.is_floating
                && !p.is_suppressed
                && p.plugin_url.as_ref() == Some(own_url)
        })
        .map(|(position, _)| *position)
}

/// New layer states from `TabUpdate` (`(position, floating visible)` per tab). The tab with
/// a shown picker keeps its previous value, since opening the picker showed its layer.
pub fn record_layers(
    previous: &LayerStates,
    tabs: &[(usize, bool)],
    picker_position: Option<usize>,
) -> LayerStates {
    tabs.iter()
        .filter_map(|&(position, visible)| {
            if Some(position) == picker_position {
                previous.get(&position).map(|&v| (position, v))
            } else {
                Some((position, visible))
            }
        })
        .collect()
}

/// Name of the layer state file in the status directory, per Zellij server.
/// Not `.json`, so the status file poll skips it.
pub fn layer_file_name(zellij_pid: u32) -> String {
    format!("floating-{zellij_pid}.state")
}

pub fn encode_layers(layers: &LayerStates) -> String {
    serde_json::to_string(layers).unwrap_or_default()
}

pub fn decode_layers(text: &str) -> Option<LayerStates> {
    serde_json::from_str(text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: u32 = 7;

    fn geometry(x: usize) -> Geometry {
        Geometry {
            x,
            y: 2,
            cols: 40,
            rows: 10,
        }
    }

    fn pane(id: u32, is_plugin: bool, is_floating: bool, is_suppressed: bool) -> PaneSnapshot {
        PaneSnapshot {
            id,
            is_plugin,
            is_floating,
            is_suppressed,
            plugin_url: is_plugin.then(|| "file:/p.wasm".to_string()),
            geometry: geometry(id as usize),
        }
    }

    #[test]
    fn hides_other_visible_floating_panes_when_layer_was_hidden() {
        let panes = [
            pane(OWN, true, true, false),
            pane(1, false, true, false),
            pane(2, true, true, false),
        ];

        let hidden = panes_to_isolate(OWN, &panes, false);

        assert_eq!(
            hidden,
            vec![
                HiddenPane {
                    id: 1,
                    is_plugin: false,
                    geometry: geometry(1),
                },
                HiddenPane {
                    id: 2,
                    is_plugin: true,
                    geometry: geometry(2),
                },
            ]
        );
    }

    #[test]
    fn keeps_terminal_with_same_id_as_picker() {
        let panes = [pane(OWN, true, true, false), pane(OWN, false, true, false)];

        let hidden = panes_to_isolate(OWN, &panes, false);

        assert_eq!(hidden.len(), 1);
        assert!(!hidden[0].is_plugin);
    }

    #[test]
    fn skips_tiled_and_suppressed_panes() {
        let panes = [pane(1, false, false, false), pane(2, false, true, true)];

        let hidden = panes_to_isolate(OWN, &panes, false);

        assert!(hidden.is_empty());
    }

    #[test]
    fn hides_nothing_when_layer_was_visible() {
        let panes = [pane(OWN, true, true, false), pane(1, false, true, false)];

        let hidden = panes_to_isolate(OWN, &panes, true);

        assert!(hidden.is_empty());
    }

    #[test]
    fn finds_shown_picker_of_same_url() {
        let mut other = pane(9, true, true, false);
        other.plugin_url = Some("zellij:strider".into());
        let panes = [
            (0, pane(1, true, false, false)),
            (0, other),
            (1, pane(OWN, true, true, false)),
        ];

        assert_eq!(shown_picker_position(1, &panes), Some(1));
    }

    #[test]
    fn hidden_picker_is_not_shown() {
        let panes = [
            (0, pane(1, true, false, false)),
            (1, pane(OWN, true, false, true)),
        ];

        assert_eq!(shown_picker_position(1, &panes), None);
    }

    #[test]
    fn picker_tab_keeps_previous_layer_state() {
        let previous = LayerStates::from([(0, false), (1, true)]);

        let layers = record_layers(&previous, &[(0, true), (1, false), (2, true)], Some(0));

        assert_eq!(
            layers,
            LayerStates::from([(0, false), (1, false), (2, true)])
        );
    }

    #[test]
    fn picker_tab_without_previous_state_is_left_out() {
        let layers = record_layers(&LayerStates::new(), &[(0, true)], Some(0));

        assert!(layers.is_empty());
    }

    #[test]
    fn layer_states_round_trip() {
        let layers = LayerStates::from([(0, false), (3, true)]);

        assert_eq!(decode_layers(&encode_layers(&layers)), Some(layers));
        assert_eq!(decode_layers("garbage"), None);
    }

    #[test]
    fn detects_show_and_move_transitions() {
        assert!(became_visible(Some(true), Some(false)));
        assert!(became_visible(None, Some(false)));
        assert!(!became_visible(Some(false), Some(false)));
        assert!(!became_visible(Some(false), Some(true)));

        assert!(moved_tab(Some((1, false)), Some((0, false))));
        assert!(!moved_tab(Some((0, false)), Some((0, false))));
        assert!(!moved_tab(Some((1, true)), Some((0, false))));
        assert!(!moved_tab(None, Some((0, false))));
    }
}

//! CC4.0 FlexiKey profile model and wire-compatible XML codec.
//!
//! The vendor stores profiles in `C:\ComboKey\profile.xml`. The model keeps
//! unknown values explicit and preserves the original action semantics so a
//! profile can be exported without silently changing launch/text bindings.

use std::fmt;
use std::fmt::Write as _;

use crate::{FilterMacro, FilterRecord};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum FlexiKeyDevice {
    #[default]
    None = 0,
    Keyboard = 1,
    Mouse = 2,
}

impl FlexiKeyDevice {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_i64(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Keyboard),
            2 => Some(Self::Mouse),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MacroStepKind {
    Make,
    Break,
}

impl MacroStepKind {
    fn as_xml(self) -> &'static str {
        match self {
            Self::Make => "KeyMake",
            Self::Break => "KeyBreak",
        }
    }

    fn from_xml(value: &str) -> Option<Self> {
        match value {
            "KeyMake" => Some(Self::Make),
            "KeyBreak" => Some(Self::Break),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroStep {
    pub key_code: i64,
    pub kind: MacroStepKind,
    pub delay_ms: i64,
    pub group: i64,
    pub display: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlexiKeyAction {
    Keypress {
        steps: Vec<MacroStep>,
    },
    Launch {
        app_path: String,
        event_id: i64,
    },
    Text {
        start_keys: Vec<MacroStep>,
        text: String,
        send_keys: Vec<MacroStep>,
    },
    Disable,
}

impl FlexiKeyAction {
    pub const fn type_id(&self) -> u8 {
        match self {
            Self::Keypress { .. } => 1,
            Self::Launch { .. } => 2,
            Self::Text { .. } => 3,
            Self::Disable => 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlexiKeyBinding {
    pub device: FlexiKeyDevice,
    pub key_code: i64,
    pub key_name: String,
    pub key_display: String,
    pub tip: String,
    pub checkbox_name: String,
    pub is_keypress_delay: bool,
    pub action: FlexiKeyAction,
}

impl FlexiKeyBinding {
    /// Convert a keyboard keypress binding to the filter driver's macro frame.
    /// Launch, text and disable actions are rejected because they require the
    /// user-mode action engine rather than a pure key replay.
    pub fn to_filter_macro(&self) -> Result<FilterMacro, FlexiKeyError> {
        if self.device != FlexiKeyDevice::Keyboard {
            return Err(FlexiKeyError::UnsupportedAction(
                "only keyboard bindings can be sent to HKKbdFltr".to_owned(),
            ));
        }
        let FlexiKeyAction::Keypress { steps } = &self.action else {
            return Err(FlexiKeyError::UnsupportedAction(
                "only keypress bindings can be sent to HKKbdFltr".to_owned(),
            ));
        };
        if steps.is_empty() {
            return Err(FlexiKeyError::InvalidBinding(
                "a keypress binding has no macro steps".to_owned(),
            ));
        }
        let trigger_key = u16::try_from(self.key_code).map_err(|_| {
            FlexiKeyError::InvalidBinding(format!(
                "trigger key {} does not fit the filter driver's 16-bit field",
                self.key_code
            ))
        })?;
        let actions = steps
            .iter()
            .map(|step| {
                let key = u16::try_from(step.key_code).map_err(|_| {
                    FlexiKeyError::InvalidBinding(format!(
                        "macro key {} does not fit the filter driver's 16-bit field",
                        step.key_code
                    ))
                })?;
                let delay = u32::try_from(step.delay_ms.max(0)).map_err(|_| {
                    FlexiKeyError::InvalidBinding(format!(
                        "macro delay {} is too large",
                        step.delay_ms
                    ))
                })?;
                Ok(match step.kind {
                    MacroStepKind::Make => FilterRecord::macro_down(key, delay),
                    MacroStepKind::Break => FilterRecord::macro_up(key, delay),
                })
            })
            .collect::<Result<Vec<_>, FlexiKeyError>>()?;
        Ok(FilterMacro {
            trigger_key,
            actions,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlexiKeyProfile {
    pub index: usize,
    pub name: String,
    pub icon: String,
    pub current_device: i64,
    pub keyboard_enabled: bool,
    pub mouse_enabled: bool,
    pub selected: bool,
    pub bindings: Vec<FlexiKeyBinding>,
}

impl FlexiKeyProfile {
    pub fn filter_macros(&self) -> Result<Vec<FilterMacro>, FlexiKeyError> {
        self.bindings
            .iter()
            .filter(|binding| matches!(binding.action, FlexiKeyAction::Keypress { .. }))
            .map(FlexiKeyBinding::to_filter_macro)
            .collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FlexiKeyStore {
    pub profiles: Vec<FlexiKeyProfile>,
}

impl FlexiKeyStore {
    pub fn active(&self) -> Option<&FlexiKeyProfile> {
        self.profiles.iter().find(|profile| profile.selected)
    }

    pub fn active_mut(&mut self) -> Option<&mut FlexiKeyProfile> {
        self.profiles.iter_mut().find(|profile| profile.selected)
    }

    /// Select by vector position. Callers that only have the vendor profile
    /// index should use [`Self::select_by_profile_index`].
    pub fn select(&mut self, position: usize) -> bool {
        let Some(target) = self.profiles.get(position) else {
            return false;
        };
        let target_index = target.index;
        for profile in &mut self.profiles {
            profile.selected = profile.index == target_index;
        }
        true
    }

    /// Select by the persistent vendor `Profile_N` index. Unlike a vector
    /// position, this index is not renumbered when an earlier profile is
    /// deleted, so it is the correct value to pass through the UI callback.
    pub fn select_by_profile_index(&mut self, profile_index: usize) -> bool {
        let Some(position) = self
            .profiles
            .iter()
            .position(|profile| profile.index == profile_index)
        else {
            return false;
        };
        self.select(position)
    }

    pub fn add_profile(&mut self, name: String) -> usize {
        let index = self
            .profiles
            .iter()
            .map(|profile| profile.index)
            .max()
            .unwrap_or(0)
            + 1;
        for profile in &mut self.profiles {
            profile.selected = false;
        }
        self.profiles.push(FlexiKeyProfile {
            index,
            name,
            icon: String::new(),
            current_device: 1,
            keyboard_enabled: true,
            mouse_enabled: true,
            selected: true,
            bindings: Vec::new(),
        });
        self.profiles.len() - 1
    }

    pub fn remove_profile(&mut self, profile_index: usize) -> bool {
        if self.profiles.len() <= 1 || profile_index >= self.profiles.len() {
            return false;
        }
        let removed_selected = self.profiles[profile_index].selected;
        self.profiles.remove(profile_index);
        if removed_selected && let Some(first) = self.profiles.first_mut() {
            first.selected = true;
        }
        true
    }

    pub fn rename_profile(&mut self, profile_index: usize, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let Some(profile) = self.profiles.get_mut(profile_index) else {
            return false;
        };
        profile.name = name.to_owned();
        true
    }

    pub fn add_binding(&mut self, profile_index: usize, binding: FlexiKeyBinding) -> bool {
        let Some(profile) = self.profiles.get_mut(profile_index) else {
            return false;
        };
        profile.bindings.push(binding);
        true
    }

    pub fn replace_binding(
        &mut self,
        profile_index: usize,
        binding_index: usize,
        binding: FlexiKeyBinding,
    ) -> bool {
        let Some(current) = self
            .profiles
            .get_mut(profile_index)
            .and_then(|profile| profile.bindings.get_mut(binding_index))
        else {
            return false;
        };
        *current = binding;
        true
    }

    pub fn remove_binding(&mut self, profile_index: usize, binding_index: usize) -> bool {
        let Some(profile) = self.profiles.get_mut(profile_index) else {
            return false;
        };
        if binding_index >= profile.bindings.len() {
            return false;
        }
        profile.bindings.remove(binding_index);
        true
    }

    pub fn from_xml(xml: &str) -> Result<Self, FlexiKeyError> {
        let document = roxmltree::Document::parse(xml)
            .map_err(|error| FlexiKeyError::Xml(error.to_string()))?;
        let root = document
            .root()
            .children()
            .find(|node| node.is_element() && node.tag_name().name() == "root")
            .ok_or_else(|| FlexiKeyError::Xml("missing <root> element".to_owned()))?;

        let mut profiles = Vec::new();
        for (index, node) in root.children().filter(|node| node.is_element()).enumerate() {
            let name = attr(node, "Name").unwrap_or("Profile").to_owned();
            let bindings = node
                .children()
                .filter(|child| child.is_element() && child.tag_name().name().starts_with("Set_"))
                .filter_map(|child| parse_binding(child).ok())
                .collect::<Vec<_>>();
            profiles.push(FlexiKeyProfile {
                index,
                name,
                icon: attr(node, "Icon").unwrap_or("").to_owned(),
                current_device: attr_i64(node, "CurrentDevice").unwrap_or_default(),
                keyboard_enabled: attr_bool(node, "IsKbEnable").unwrap_or(true),
                mouse_enabled: attr_bool(node, "IsMouseEnable").unwrap_or(true),
                selected: attr_bool(node, "IsSelected").unwrap_or(false),
                bindings,
            });
        }
        if !profiles.iter().any(|profile| profile.selected)
            && let Some(first) = profiles.first_mut()
        {
            first.selected = true;
        }
        Ok(Self { profiles })
    }

    pub fn to_xml(&self) -> String {
        let active = self.active().map(|profile| profile.index);
        let mut output =
            String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<root");
        if let Some(active) = active {
            let _ = write!(output, " Active=\"{active}\"");
        }
        output.push_str(">\n");
        for profile in &self.profiles {
            let _ = writeln!(
                output,
                "  <Profile_{} Name=\"{}\" IsSelected=\"{}\" CurrentDevice=\"{}\" IsKbEnable=\"{}\" IsMouseEnable=\"{}\" Icon=\"{}\">",
                profile.index,
                xml_escape(&profile.name),
                u8::from(profile.selected),
                profile.current_device,
                u8::from(profile.keyboard_enabled),
                u8::from(profile.mouse_enabled),
                xml_escape(&profile.icon)
            );
            for (binding_index, binding) in profile.bindings.iter().enumerate() {
                write_binding(&mut output, binding_index + 1, binding);
            }
            let _ = writeln!(output, "  </Profile_{}>", profile.index);
        }
        output.push_str("</root>\n");
        output
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlexiKeyError {
    Xml(String),
    InvalidBinding(String),
    UnsupportedAction(String),
}

impl fmt::Display for FlexiKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Xml(detail) => write!(formatter, "FlexiKey XML error: {detail}"),
            Self::InvalidBinding(detail) => write!(formatter, "invalid FlexiKey binding: {detail}"),
            Self::UnsupportedAction(detail) => {
                write!(formatter, "unsupported FlexiKey action: {detail}")
            }
        }
    }
}

impl std::error::Error for FlexiKeyError {}

fn parse_binding(node: roxmltree::Node<'_, '_>) -> Result<FlexiKeyBinding, FlexiKeyError> {
    let device = FlexiKeyDevice::from_i64(required_i64(node, "Device")?)
        .ok_or_else(|| FlexiKeyError::InvalidBinding("unknown device type".to_owned()))?;
    let action_type = required_i64(node, "Action")?;
    let mut steps = Vec::new();
    match action_type {
        1 => {
            for child in node
                .children()
                .filter(|child| child.is_element() && child.tag_name().name().starts_with("Index_"))
            {
                steps.push(parse_macro_step(child, true)?);
            }
            if steps.is_empty() {
                return Err(FlexiKeyError::InvalidBinding(
                    "keypress action has no Index_ entries".to_owned(),
                ));
            }
        }
        4 => {}
        2 | 3 => {}
        _ => {
            return Err(FlexiKeyError::InvalidBinding(format!(
                "unknown action type {action_type}"
            )));
        }
    }

    let action = match action_type {
        1 => FlexiKeyAction::Keypress { steps },
        2 => {
            let child = node
                .children()
                .find(|child| child.is_element())
                .ok_or_else(|| {
                    FlexiKeyError::InvalidBinding("launch action has no entry".to_owned())
                })?;
            FlexiKeyAction::Launch {
                app_path: attr(child, "AppPath").unwrap_or("").to_owned(),
                event_id: attr_i64(child, "AppEventId").unwrap_or_default(),
            }
        }
        3 => {
            let mut start_keys = Vec::new();
            let mut text = String::new();
            let mut send_keys = Vec::new();
            for child in node.children().filter(|child| child.is_element()) {
                match child.tag_name().name() {
                    "StartKey" => start_keys.push(parse_macro_step(child, false)?),
                    name if name.starts_with("Text") => {
                        text = attr(child, "TextContent").unwrap_or("").to_owned();
                    }
                    "SendKey" => send_keys.push(parse_macro_step(child, false)?),
                    _ => {}
                }
            }
            FlexiKeyAction::Text {
                start_keys,
                text,
                send_keys,
            }
        }
        4 => FlexiKeyAction::Disable,
        _ => unreachable!(),
    };

    Ok(FlexiKeyBinding {
        device,
        key_code: required_i64(node, "KeyCode")?,
        key_name: attr(node, "UiName").unwrap_or("").to_owned(),
        key_display: attr(node, "KeyDisplay").unwrap_or("").to_owned(),
        tip: attr(node, "Tip").unwrap_or("").to_owned(),
        checkbox_name: attr(node, "CheckBoxName").unwrap_or("").to_owned(),
        is_keypress_delay: attr_bool(node, "IsKeypressDelay").unwrap_or(false),
        action,
    })
}

fn parse_macro_step(
    node: roxmltree::Node<'_, '_>,
    include_group: bool,
) -> Result<MacroStep, FlexiKeyError> {
    let kind = attr(node, "KeyMakeBreak")
        .and_then(MacroStepKind::from_xml)
        .ok_or_else(|| FlexiKeyError::InvalidBinding("missing KeyMakeBreak".to_owned()))?;
    Ok(MacroStep {
        key_code: required_i64(node, "KeyCode")?,
        kind,
        delay_ms: attr_i64(node, "KeyDelay").unwrap_or_default(),
        group: if include_group {
            attr_i64(node, "KeysGroup").unwrap_or_default()
        } else {
            0
        },
        display: attr(node, "KeyDisplay").unwrap_or("").to_owned(),
    })
}

fn write_binding(output: &mut String, index: usize, binding: &FlexiKeyBinding) {
    let _ = writeln!(
        output,
        "    <Set_{} Device=\"{}\" KeyCode=\"{}\" UiName=\"{}\" KeyDisplay=\"{}\" Action=\"{}\" IsKeypressDelay=\"{}\" Tip=\"{}\" CheckBoxName=\"{}\">",
        index,
        binding.device.as_u8(),
        binding.key_code,
        xml_escape(&binding.key_name),
        xml_escape(&binding.key_display),
        binding.action.type_id(),
        u8::from(binding.is_keypress_delay),
        xml_escape(&binding.tip),
        xml_escape(&binding.checkbox_name)
    );
    match &binding.action {
        FlexiKeyAction::Keypress { steps } => {
            for (step_index, step) in steps.iter().enumerate() {
                write_macro_step(output, step_index, "Index", step);
            }
        }
        FlexiKeyAction::Launch { app_path, event_id } => {
            let _ = writeln!(
                output,
                "      <Index_0 AppPath=\"{}\" AppEventId=\"{event_id}\"/>",
                xml_escape(app_path)
            );
        }
        FlexiKeyAction::Text {
            start_keys,
            text,
            send_keys,
        } => {
            for step in start_keys {
                write_macro_step(output, 0, "StartKey", step);
            }
            let _ = writeln!(output, "      <Text TextContent=\"{}\"/>", xml_escape(text));
            for step in send_keys {
                write_macro_step(output, 0, "SendKey", step);
            }
        }
        FlexiKeyAction::Disable => {}
    }
    let _ = writeln!(output, "    </Set_{index}>");
}

fn write_macro_step(output: &mut String, index: usize, element: &str, step: &MacroStep) {
    if element == "Index" {
        let _ = writeln!(
            output,
            "      <Index_{index} KeyCode=\"{}\" KeysGroup=\"{}\" KeyMakeBreak=\"{}\" KeyDelay=\"{}\" KeyDisplay=\"{}\"/>",
            step.key_code,
            step.group,
            step.kind.as_xml(),
            step.delay_ms,
            xml_escape(&step.display)
        );
    } else {
        let _ = writeln!(
            output,
            "      <{element} KeyCode=\"{}\" KeyMakeBreak=\"{}\" KeyDelay=\"{}\" KeyDisplay=\"{}\"/>",
            step.key_code,
            step.kind.as_xml(),
            step.delay_ms,
            xml_escape(&step.display)
        );
    }
}

fn attr<'a>(node: roxmltree::Node<'a, 'a>, name: &str) -> Option<&'a str> {
    node.attribute(name)
}

fn attr_i64(node: roxmltree::Node<'_, '_>, name: &str) -> Option<i64> {
    attr(node, name)?.trim().parse().ok()
}

fn attr_bool(node: roxmltree::Node<'_, '_>, name: &str) -> Option<bool> {
    match attr_i64(node, name)? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn required_i64(node: roxmltree::Node<'_, '_>, name: &str) -> Result<i64, FlexiKeyError> {
    attr_i64(node, name)
        .ok_or_else(|| FlexiKeyError::InvalidBinding(format!("missing numeric {name} attribute")))
}

fn xml_escape(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&apos;"),
            _ => output.push(character),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<root Active="0">
  <Profile_0 Name="Gaming &amp; Work" IsSelected="1" CurrentDevice="1" IsKbEnable="1" IsMouseEnable="0" Icon="game.png">
    <Set_1 Device="1" KeyCode="57431" UiName="F10" KeyDisplay="F10" Action="1" IsKeypressDelay="0" Tip="tip" CheckBoxName="cb">
      <Index_0 KeyCode="57432" KeysGroup="0" KeyMakeBreak="KeyMake" KeyDelay="10" KeyDisplay="F11"/>
      <Index_1 KeyCode="57432" KeysGroup="0" KeyMakeBreak="KeyBreak" KeyDelay="10" KeyDisplay="F11"/>
    </Set_1>
    <Set_2 Device="1" KeyCode="57433" UiName="F12" KeyDisplay="F12" Action="2" IsKeypressDelay="0" Tip="" CheckBoxName="">
      <Index_0 AppPath="C:\Tools\app.exe" AppEventId="7"/>
    </Set_2>
    <Set_3 Device="1" KeyCode="57434" UiName="F9" KeyDisplay="F9" Action="3" IsKeypressDelay="1" Tip="" CheckBoxName="">
      <StartKey KeyCode="57435" KeyMakeBreak="KeyMake" KeyDelay="5" KeyDisplay="A"/>
      <Text TextContent="hello &amp; bye"/>
      <SendKey KeyCode="57436" KeyMakeBreak="KeyBreak" KeyDelay="7" KeyDisplay="B"/>
    </Set_3>
    <Set_4 Device="1" KeyCode="57437" UiName="F8" KeyDisplay="F8" Action="4" IsKeypressDelay="0" Tip="" CheckBoxName=""/>
  </Profile_0>
</root>"#;

    #[test]
    fn parses_all_vendor_action_types() {
        let store = FlexiKeyStore::from_xml(SAMPLE).expect("sample must parse");
        let profile = store.active().expect("active profile");
        assert_eq!(profile.name, "Gaming & Work");
        assert_eq!(profile.bindings.len(), 4);
        assert!(matches!(
            profile.bindings[0].action,
            FlexiKeyAction::Keypress { .. }
        ));
        assert!(matches!(
            profile.bindings[1].action,
            FlexiKeyAction::Launch { .. }
        ));
        assert!(matches!(
            profile.bindings[2].action,
            FlexiKeyAction::Text { .. }
        ));
        assert!(matches!(
            profile.bindings[3].action,
            FlexiKeyAction::Disable
        ));
    }

    #[test]
    fn xml_round_trip_preserves_profiles_and_bindings() {
        let store = FlexiKeyStore::from_xml(SAMPLE).expect("sample must parse");
        let encoded = store.to_xml();
        let decoded = FlexiKeyStore::from_xml(&encoded).expect("round-trip XML must parse");
        assert_eq!(store, decoded);
    }

    #[test]
    fn malformed_binding_is_skipped_without_losing_profile() {
        let xml = r#"<root><Profile_0 Name="P" IsSelected="1">
            <Set_1 Device="1" KeyCode="1" Action="9"/>
            <Set_2 Device="1" KeyCode="57463" Action="4"/>
        </Profile_0></root>"#;
        let store = FlexiKeyStore::from_xml(xml).expect("profile must parse");
        assert_eq!(store.profiles[0].bindings.len(), 1);
        assert!(matches!(
            store.profiles[0].bindings[0].action,
            FlexiKeyAction::Disable
        ));
    }

    #[test]
    fn keypress_binding_converts_to_filter_macro() {
        let store = FlexiKeyStore::from_xml(SAMPLE).expect("sample must parse");
        let macro_value = store.profiles[0].filter_macros().expect("keypress macro");
        assert_eq!(macro_value.len(), 1);
        assert_eq!(macro_value[0].trigger_key, 57431);
        assert_eq!(macro_value[0].actions.len(), 2);
        assert_eq!(macro_value[0].actions[0].kind, 1);
        assert_eq!(macro_value[0].actions[1].kind, 2);
    }

    #[test]
    fn active_profile_can_be_changed() {
        let xml = r#"<root>
            <Profile_0 Name="A" IsSelected="1"/>
            <Profile_1 Name="B" IsSelected="0"/>
        </root>"#;
        let mut store = FlexiKeyStore::from_xml(xml).expect("profiles must parse");
        assert!(store.select(1));
        assert_eq!(
            store.active().map(|profile| profile.name.as_str()),
            Some("B")
        );
        assert!(!store.profiles[0].selected);
    }

    #[test]
    fn selecting_by_profile_index_survives_a_deleted_middle_profile() {
        let xml = r#"<root>
            <Profile_0 Name="A" IsSelected="1"/>
            <Profile_1 Name="B" IsSelected="0"/>
            <Profile_2 Name="C" IsSelected="0"/>
        </root>"#;
        let mut store = FlexiKeyStore::from_xml(xml).expect("profiles must parse");
        assert!(store.remove_profile(1));
        assert_eq!(
            store
                .profiles
                .iter()
                .map(|profile| profile.index)
                .collect::<Vec<_>>(),
            vec![0, 2]
        );
        assert!(store.select_by_profile_index(2));
        assert_eq!(
            store.active().map(|profile| profile.name.as_str()),
            Some("C")
        );
        assert!(!store.select_by_profile_index(1));
    }

    #[test]
    fn bindings_can_be_added_replaced_and_removed() {
        let xml = r#"<root>
            <Profile_0 Name="A" IsSelected="1">
              <Set_1 Device="1" KeyCode="30" Action="4"/>
            </Profile_0>
        </root>"#;
        let mut store = FlexiKeyStore::from_xml(xml).expect("profile must parse");
        let replacement = FlexiKeyBinding {
            device: FlexiKeyDevice::Mouse,
            key_code: 0x40,
            key_name: "Back".to_owned(),
            key_display: "Mouse Back".to_owned(),
            tip: String::new(),
            checkbox_name: String::new(),
            is_keypress_delay: false,
            action: FlexiKeyAction::Disable,
        };

        assert!(store.add_binding(0, replacement.clone()));
        assert_eq!(store.profiles[0].bindings.len(), 2);
        assert!(store.replace_binding(0, 0, replacement.clone()));
        assert_eq!(store.profiles[0].bindings[0], replacement);
        assert!(store.remove_binding(0, 0));
        assert_eq!(store.profiles[0].bindings.len(), 1);
        assert!(!store.remove_binding(0, 8));
        assert!(!store.add_binding(8, replacement));
    }

    #[test]
    fn profiles_can_be_added_renamed_and_removed() {
        let xml = r#"<root><Profile_0 Name="A" IsSelected="1"/></root>"#;
        let mut store = FlexiKeyStore::from_xml(xml).expect("profile must parse");
        let position = store.add_profile("B".to_owned());
        assert_eq!(position, 1);
        assert_eq!(
            store.active().map(|profile| profile.name.as_str()),
            Some("B")
        );
        assert!(store.rename_profile(position, "Work"));
        assert!(!store.rename_profile(position, "  "));
        assert_eq!(
            store.active().map(|profile| profile.name.as_str()),
            Some("Work")
        );
        assert!(store.remove_profile(position));
        assert_eq!(store.profiles.len(), 1);
        assert!(store.active().is_some());
        assert!(!store.remove_profile(0));
    }
}

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use neo_control_protocol::{
    FlexiKeyAction, FlexiKeyBinding, FlexiKeyDevice, FlexiKeyProfile, FlexiKeyStore,
};
use slint::{ComponentHandle, VecModel, Weak};

use crate::MainWindow;
use crate::diagnostics::log_action;
use crate::flexikey_engine::{self, EngineReport, FlexiKeyEngine};
use crate::localization::tr;

pub(crate) struct FlexiKeyViewState {
    store: FlexiKeyStore,
    path: PathBuf,
    load_error: Option<String>,
    activated_index: Option<usize>,
    operation_error: Option<String>,
    operation_notice: Option<String>,
    engine_report: Option<EngineReport>,
    engine_error: Option<String>,
}

fn flexikey_profile_path() -> PathBuf {
    if let Some(path) = std::env::var_os("NEO_CONTROL_FLEXIKEY_XML") {
        return PathBuf::from(path);
    }
    let system_drive = std::env::var_os("SystemDrive").unwrap_or_else(|| "C:".into());
    PathBuf::from(system_drive)
        .join("ComboKey")
        .join("profile.xml")
}

fn blank_flexikey_store() -> FlexiKeyStore {
    FlexiKeyStore {
        profiles: vec![FlexiKeyProfile {
            index: 0,
            name: tr("Personal", "个人配置").to_owned(),
            icon: String::new(),
            current_device: 1,
            keyboard_enabled: true,
            mouse_enabled: true,
            selected: true,
            bindings: Vec::new(),
        }],
    }
}

fn load_flexikey_view(path: &Path) -> FlexiKeyViewState {
    match std::fs::read_to_string(path) {
        Ok(xml) => match FlexiKeyStore::from_xml(&xml) {
            Ok(store) => FlexiKeyViewState {
                store,
                path: path.to_owned(),
                load_error: None,
                activated_index: None,
                operation_error: None,
                operation_notice: None,
                engine_report: None,
                engine_error: None,
            },
            Err(error) => FlexiKeyViewState {
                store: FlexiKeyStore::default(),
                path: path.to_owned(),
                load_error: Some(error.to_string()),
                activated_index: None,
                operation_error: None,
                operation_notice: None,
                engine_report: None,
                engine_error: None,
            },
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => FlexiKeyViewState {
            store: blank_flexikey_store(),
            path: path.to_owned(),
            load_error: None,
            activated_index: None,
            operation_error: None,
            operation_notice: Some(tr(
                "profile.xml was not found. A blank profile is ready; saving a binding creates the file.",
                "未找到 profile.xml。已准备空白配置，保存绑定时会创建此文件。",
            )
            .to_owned()),
            engine_report: None,
            engine_error: None,
        },
        Err(error) => FlexiKeyViewState {
            store: FlexiKeyStore::default(),
            path: path.to_owned(),
            load_error: Some(error.to_string()),
            activated_index: None,
            operation_error: None,
            operation_notice: None,
            engine_report: None,
            engine_error: None,
        },
    }
}

fn sync_flexikey_engine(
    engine: &FlexiKeyEngine,
    store: &FlexiKeyStore,
) -> (Option<EngineReport>, Option<String>) {
    match engine.update_profile(store.active()) {
        Ok(report) => (Some(report), None),
        Err(error) => (None, Some(error)),
    }
}

fn active_profile_position(store: &FlexiKeyStore) -> Option<usize> {
    store.profiles.iter().position(|profile| profile.selected)
}

fn parse_trigger_key_code(value: &str) -> Result<u16, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(tr("Enter or capture a trigger key", "请选择或输入触发按键").to_owned());
    }
    let parsed = if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u16::from_str_radix(hex, 16)
    } else {
        value.parse::<u16>()
    };
    parsed.map_err(|_| {
        format!(
            "{}: {value}",
            tr("Invalid 16-bit key code", "无效的 16 位按键代码")
        )
    })
}

fn mouse_trigger_name(key_code: u16) -> String {
    let name = match key_code {
        0x01 => tr("Mouse Left", "鼠标左键"),
        0x04 => tr("Mouse Right", "鼠标右键"),
        0x10 => tr("Mouse Middle", "鼠标中键"),
        0x40 => tr("Mouse Back", "鼠标后退"),
        0x100 => tr("Mouse Forward", "鼠标前进"),
        _ => tr("Mouse Button", "鼠标按钮"),
    };
    name.to_owned()
}

fn editor_action(
    existing: Option<&FlexiKeyAction>,
    action_type: i32,
    value: &str,
) -> Result<FlexiKeyAction, String> {
    match action_type {
        1 => {
            if value.trim().is_empty() {
                if let Some(action @ FlexiKeyAction::Keypress { .. }) = existing {
                    return Ok(action.clone());
                }
                return Err(tr(
                    "Enter a hotkey sequence for the new binding",
                    "请为新绑定输入快捷键组合",
                )
                .to_owned());
            }
            Ok(FlexiKeyAction::Keypress {
                steps: flexikey_engine::hotkey_steps(value)?,
            })
        }
        2 => {
            let app_path = value.trim();
            if app_path.is_empty() {
                return Err(tr("Enter a launch target", "请输入要启动的程序或文档").to_owned());
            }
            let event_id = match existing {
                Some(FlexiKeyAction::Launch { event_id, .. }) => *event_id,
                _ => 0,
            };
            Ok(FlexiKeyAction::Launch {
                app_path: app_path.to_owned(),
                event_id,
            })
        }
        3 => {
            let (start_keys, send_keys) = match existing {
                Some(FlexiKeyAction::Text {
                    start_keys,
                    send_keys,
                    ..
                }) => (start_keys.clone(), send_keys.clone()),
                _ => (Vec::new(), Vec::new()),
            };
            Ok(FlexiKeyAction::Text {
                start_keys,
                text: value.to_owned(),
                send_keys,
            })
        }
        4 => Ok(FlexiKeyAction::Disable),
        _ => Err(tr("Unknown action type", "未知的动作类型").to_owned()),
    }
}

fn editor_binding(
    profile: &FlexiKeyProfile,
    binding_index: i32,
    device: i32,
    key_code: &str,
    key_name: &str,
    action_type: i32,
    value: &str,
) -> Result<FlexiKeyBinding, String> {
    let device = match device {
        1 => FlexiKeyDevice::Keyboard,
        2 => FlexiKeyDevice::Mouse,
        _ => return Err(tr("Choose a trigger device", "请选择触发设备").to_owned()),
    };
    let key_code = parse_trigger_key_code(key_code)?;
    if device == FlexiKeyDevice::Mouse && !matches!(key_code, 0x01 | 0x04 | 0x10 | 0x40 | 0x100) {
        return Err(tr(
            "Mouse triggers must be left, right, middle, back or forward",
            "鼠标触发只支持左键、右键、中键、后退和前进",
        )
        .to_owned());
    }
    let existing = usize::try_from(binding_index)
        .ok()
        .and_then(|index| profile.bindings.get(index));
    let display = if key_name.trim().is_empty() {
        if device == FlexiKeyDevice::Mouse {
            mouse_trigger_name(key_code)
        } else {
            format!("Key 0x{key_code:04X}")
        }
    } else {
        key_name.trim().to_owned()
    };
    Ok(FlexiKeyBinding {
        device,
        key_code: i64::from(key_code),
        key_name: display.clone(),
        key_display: display,
        tip: existing
            .map(|binding| binding.tip.clone())
            .unwrap_or_default(),
        checkbox_name: existing
            .map(|binding| binding.checkbox_name.clone())
            .unwrap_or_default(),
        is_keypress_delay: existing
            .map(|binding| binding.is_keypress_delay)
            .unwrap_or(false),
        action: editor_action(existing.map(|binding| &binding.action), action_type, value)?,
    })
}

fn format_binding_label(index: usize, binding: &FlexiKeyBinding) -> String {
    let device = match binding.device {
        FlexiKeyDevice::Keyboard => tr("Keyboard", "键盘"),
        FlexiKeyDevice::Mouse => tr("Mouse", "鼠标"),
        FlexiKeyDevice::None => "---",
    };
    let trigger = if binding.key_display.trim().is_empty() {
        if binding.device == FlexiKeyDevice::Mouse {
            mouse_trigger_name(binding.key_code as u16)
        } else {
            "Key".to_owned()
        }
    } else {
        binding.key_display.trim().to_owned()
    };
    let action = match &binding.action {
        FlexiKeyAction::Keypress { steps } => {
            format!(
                "{} ({} {})",
                tr("Hotkey", "快捷键"),
                steps.len(),
                tr("steps", "步")
            )
        }
        FlexiKeyAction::Launch { app_path, .. } => {
            format!("{} {}", tr("Launch", "启动"), app_path)
        }
        FlexiKeyAction::Text { text, .. } => {
            let preview = text.chars().take(36).collect::<String>();
            if text.chars().count() > 36 {
                format!("{} \"{preview}…\"", tr("Text", "文本"))
            } else {
                format!("{} \"{preview}\"", tr("Text", "文本"))
            }
        }
        FlexiKeyAction::Disable => tr("Disabled trigger", "禁用触发").to_owned(),
    };
    format!("{:02}  {device}  {trigger:<14}  →  {action}", index + 1)
}

fn start_flexikey_capture_poll(weak: Weak<MainWindow>, engine: FlexiKeyEngine) {
    slint::Timer::single_shot(Duration::from_millis(50), move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if !ui.get_flexikey_capture_armed() {
            return;
        }
        if let Some(trigger) = engine.take_captured_trigger() {
            ui.set_flexikey_editor_device(i32::from(trigger.device));
            ui.set_flexikey_editor_key_code(format!("0x{:04X}", trigger.key_code).into());
            let name = if trigger.device == FlexiKeyDevice::Mouse.as_u8() {
                mouse_trigger_name(trigger.key_code)
            } else {
                trigger.name
            };
            ui.set_flexikey_editor_key_name(name.into());
            ui.set_flexikey_capture_armed(false);
            ui.set_flexikey_editor_status(
                tr(
                    "Trigger captured. Choose the action and save.",
                    "已捕获触发按键，请选择动作并保存。",
                )
                .into(),
            );
        } else {
            start_flexikey_capture_poll(ui.as_weak(), engine);
        }
    });
}

pub(crate) fn wire_flexikey(
    ui: &MainWindow,
    engine: FlexiKeyEngine,
) -> Rc<RefCell<FlexiKeyViewState>> {
    let path = flexikey_profile_path();
    let mut initial = load_flexikey_view(&path);
    let (engine_report, engine_error) = sync_flexikey_engine(&engine, &initial.store);
    initial.engine_report = engine_report;
    initial.engine_error = engine_error;
    let state = Rc::new(RefCell::new(initial));
    render_flexikey(ui, &state.borrow());

    let weak = ui.as_weak();
    let state_for_select = Rc::clone(&state);
    let engine_for_select = engine.clone();
    ui.on_select_flexikey_profile(move |index| {
        log_action(&format!("select-flexikey-profile callback {index}"));
        if let Some(ui) = weak.upgrade() {
            ui.set_flexikey_delete_armed(false);
        }
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        let mut state = state_for_select.borrow_mut();
        if state.store.active().map(|profile| profile.index) == Some(index) {
            state.activated_index = Some(index);
            state.operation_error = None;
            state.operation_notice = None;
            drop(state);
            if let Some(ui) = weak.upgrade() {
                render_flexikey(&ui, &state_for_select.borrow());
            }
            return;
        }
        let mut next = state.store.clone();
        if !next.select_by_profile_index(index) {
            return;
        }
        if let Err(error) = engine_for_select.update_profile(next.active()) {
            state.operation_error = Some(format!(
                "{}: {error}",
                tr("Action engine rejected the profile", "动作引擎拒绝了该配置")
            ));
            drop(state);
            if let Some(ui) = weak.upgrade() {
                render_flexikey(&ui, &state_for_select.borrow());
            }
            return;
        }
        if let Err(error) = save_flexikey_store(&state.path, &next) {
            let _ = engine_for_select.update_profile(state.store.active());
            state.operation_error = Some(format!(
                "{}: {error}",
                tr("Could not save profile.xml", "无法保存 profile.xml")
            ));
            drop(state);
            if let Some(ui) = weak.upgrade() {
                render_flexikey(&ui, &state_for_select.borrow());
            }
            return;
        }
        state.store = next;
        state.load_error = None;
        state.operation_error = None;
        state.operation_notice = None;
        state.activated_index = Some(index);
        let (report, error) = sync_flexikey_engine(&engine_for_select, &state.store);
        state.engine_report = report;
        state.engine_error = error;
        drop(state);
        if let Some(ui) = weak.upgrade() {
            render_flexikey(&ui, &state_for_select.borrow());
        }
    });

    let weak = ui.as_weak();
    let state_for_new_profile = Rc::clone(&state);
    let engine_for_new_profile = engine.clone();
    ui.on_new_flexikey_profile(move || {
        let mut next = state_for_new_profile.borrow().store.clone();
        let name = format!(
            "{} {}",
            tr("Profile", "配置"),
            next.profiles.len().saturating_add(1)
        );
        next.add_profile(name);
        match commit_flexikey_store(
            &state_for_new_profile,
            &engine_for_new_profile,
            next,
            tr(
                "A new FlexiKey profile was created and activated.",
                "新的 FlexiKey 配置已创建并启用。",
            )
            .to_owned(),
        ) {
            Ok(()) => {
                if let Some(ui) = weak.upgrade() {
                    ui.set_flexikey_delete_armed(false);
                    render_flexikey(&ui, &state_for_new_profile.borrow());
                }
            }
            Err(error) => {
                let mut state = state_for_new_profile.borrow_mut();
                state.operation_error = Some(format!(
                    "{}: {error}",
                    tr("Could not create profile", "无法创建配置")
                ));
                drop(state);
                if let Some(ui) = weak.upgrade() {
                    render_flexikey(&ui, &state_for_new_profile.borrow());
                }
            }
        }
    });

    let weak = ui.as_weak();
    let state_for_rename_profile = Rc::clone(&state);
    let engine_for_rename_profile = engine.clone();
    ui.on_rename_flexikey_profile(move |name| {
        log_action(&format!("rename-flexikey-profile callback: {name}"));
        let state_snapshot = state_for_rename_profile.borrow();
        let Some(profile_position) = active_profile_position(&state_snapshot.store) else {
            return;
        };
        let mut next = state_snapshot.store.clone();
        drop(state_snapshot);
        if !next.rename_profile(profile_position, name.as_str()) {
            let mut state = state_for_rename_profile.borrow_mut();
            state.operation_error =
                Some(tr("Profile name cannot be empty.", "配置名称不能为空。").to_owned());
            drop(state);
            if let Some(ui) = weak.upgrade() {
                render_flexikey(&ui, &state_for_rename_profile.borrow());
            }
            return;
        }
        match commit_flexikey_store(
            &state_for_rename_profile,
            &engine_for_rename_profile,
            next,
            tr("FlexiKey profile name saved.", "FlexiKey 配置名称已保存。").to_owned(),
        ) {
            Ok(()) => {
                if let Some(ui) = weak.upgrade() {
                    ui.set_flexikey_delete_armed(false);
                    render_flexikey(&ui, &state_for_rename_profile.borrow());
                }
            }
            Err(error) => {
                let mut state = state_for_rename_profile.borrow_mut();
                state.operation_error = Some(format!(
                    "{}: {error}",
                    tr("Could not rename profile", "无法重命名配置")
                ));
                drop(state);
                if let Some(ui) = weak.upgrade() {
                    render_flexikey(&ui, &state_for_rename_profile.borrow());
                }
            }
        }
    });

    let weak = ui.as_weak();
    let state_for_delete_profile = Rc::clone(&state);
    let engine_for_delete_profile = engine.clone();
    ui.on_delete_flexikey_profile(move || {
        let state_snapshot = state_for_delete_profile.borrow();
        let Some(profile_position) = active_profile_position(&state_snapshot.store) else {
            return;
        };
        let mut next = state_snapshot.store.clone();
        drop(state_snapshot);
        if !next.remove_profile(profile_position) {
            let mut state = state_for_delete_profile.borrow_mut();
            state.operation_error = Some(
                tr(
                    "At least one FlexiKey profile must remain.",
                    "必须至少保留一个 FlexiKey 配置。",
                )
                .to_owned(),
            );
            drop(state);
            if let Some(ui) = weak.upgrade() {
                render_flexikey(&ui, &state_for_delete_profile.borrow());
            }
            return;
        }
        match commit_flexikey_store(
            &state_for_delete_profile,
            &engine_for_delete_profile,
            next,
            tr(
                "FlexiKey profile deleted and the next profile activated.",
                "FlexiKey 配置已删除，并启用下一个配置。",
            )
            .to_owned(),
        ) {
            Ok(()) => {
                if let Some(ui) = weak.upgrade() {
                    ui.set_flexikey_delete_armed(false);
                    render_flexikey(&ui, &state_for_delete_profile.borrow());
                }
            }
            Err(error) => {
                let mut state = state_for_delete_profile.borrow_mut();
                state.operation_error = Some(format!(
                    "{}: {error}",
                    tr("Could not delete profile", "无法删除配置")
                ));
                drop(state);
                if let Some(ui) = weak.upgrade() {
                    render_flexikey(&ui, &state_for_delete_profile.borrow());
                }
            }
        }
    });

    let weak = ui.as_weak();
    let state_for_devices = Rc::clone(&state);
    let engine_for_devices = engine.clone();
    ui.on_set_flexikey_device_enabled(move |device, enabled| {
        let state_snapshot = state_for_devices.borrow();
        let Some(profile_position) = active_profile_position(&state_snapshot.store) else {
            return;
        };
        let mut next = state_snapshot.store.clone();
        drop(state_snapshot);
        let Some(profile) = next.profiles.get_mut(profile_position) else {
            return;
        };
        match device {
            1 => profile.keyboard_enabled = enabled,
            2 => profile.mouse_enabled = enabled,
            _ => return,
        }
        let report = match engine_for_devices.update_profile(next.active()) {
            Ok(report) => report,
            Err(error) => {
                let mut state = state_for_devices.borrow_mut();
                state.operation_error = Some(format!(
                    "{}: {error}",
                    tr("Action engine rejected the change", "动作引擎拒绝了更改")
                ));
                drop(state);
                if let Some(ui) = weak.upgrade() {
                    render_flexikey(&ui, &state_for_devices.borrow());
                }
                return;
            }
        };
        let previous = state_for_devices.borrow().store.clone();
        if let Err(error) = save_flexikey_store(&state_for_devices.borrow().path, &next) {
            let _ = engine_for_devices.update_profile(previous.active());
            let mut state = state_for_devices.borrow_mut();
            state.operation_error = Some(format!(
                "{}: {error}",
                tr("Could not save profile.xml", "无法保存 profile.xml")
            ));
            drop(state);
            if let Some(ui) = weak.upgrade() {
                render_flexikey(&ui, &state_for_devices.borrow());
            }
            return;
        }
        let mut state = state_for_devices.borrow_mut();
        state.store = next;
        state.operation_error = None;
        state.operation_notice = Some(
            tr(
                "FlexiKey trigger devices saved and reloaded.",
                "FlexiKey 触发设备已保存并重新载入。",
            )
            .to_owned(),
        );
        state.activated_index = None;
        state.engine_report = Some(report);
        state.engine_error = None;
        drop(state);
        if let Some(ui) = weak.upgrade() {
            render_flexikey(&ui, &state_for_devices.borrow());
        }
    });

    let weak = ui.as_weak();
    let state_for_open = Rc::clone(&state);
    let engine_for_open = engine.clone();
    ui.on_open_flexikey_editor(move |binding_index| {
        engine_for_open.cancel_capture();
        let state = state_for_open.borrow();
        let Some(profile) = state.store.active() else {
            return;
        };
        let binding = usize::try_from(binding_index)
            .ok()
            .and_then(|index| profile.bindings.get(index));
        if let Some(ui) = weak.upgrade() {
            if let Some(binding) = binding {
                ui.set_flexikey_editor_binding_index(binding_index);
                ui.set_flexikey_editor_device(i32::from(binding.device.as_u8()));
                ui.set_flexikey_editor_key_code(format!("0x{:04X}", binding.key_code).into());
                ui.set_flexikey_editor_key_name(binding.key_display.clone().into());
                ui.set_flexikey_editor_action(i32::from(binding.action.type_id()));
                ui.set_flexikey_editor_value(
                    match &binding.action {
                        FlexiKeyAction::Launch { app_path, .. } => app_path.clone(),
                        FlexiKeyAction::Text { text, .. } => text.clone(),
                        _ => String::new(),
                    }
                    .into(),
                );
            } else {
                ui.set_flexikey_editor_binding_index(-1);
                ui.set_flexikey_editor_device(1);
                ui.set_flexikey_editor_key_code("".into());
                ui.set_flexikey_editor_key_name("".into());
                ui.set_flexikey_editor_action(1);
                ui.set_flexikey_editor_value("".into());
            }
            ui.set_flexikey_editor_status("".into());
            ui.set_flexikey_capture_armed(false);
            ui.set_flexikey_editor_open(true);
        }
    });

    let weak = ui.as_weak();
    let engine_for_close = engine.clone();
    ui.on_close_flexikey_editor(move || {
        engine_for_close.cancel_capture();
        if let Some(ui) = weak.upgrade() {
            ui.set_flexikey_capture_armed(false);
            ui.set_flexikey_editor_status("".into());
            ui.set_flexikey_editor_open(false);
        }
    });

    let weak = ui.as_weak();
    let engine_for_capture = engine.clone();
    ui.on_capture_flexikey_trigger(move || {
        if let Some(ui) = weak.upgrade() {
            match engine_for_capture.arm_capture() {
                Ok(()) => {
                    ui.set_flexikey_editor_status(
                        tr(
                            "Listening for one key or mouse button…",
                            "正在监听一个按键或鼠标按钮…",
                        )
                        .into(),
                    );
                    ui.set_flexikey_capture_armed(true);
                    start_flexikey_capture_poll(ui.as_weak(), engine_for_capture.clone());
                }
                Err(error) => {
                    ui.set_flexikey_editor_status(error.into());
                    ui.set_flexikey_capture_armed(false);
                }
            }
        }
    });

    let weak = ui.as_weak();
    let engine_for_cancel_capture = engine.clone();
    ui.on_cancel_flexikey_capture(move || {
        engine_for_cancel_capture.cancel_capture();
        if let Some(ui) = weak.upgrade() {
            ui.set_flexikey_capture_armed(false);
            ui.set_flexikey_editor_status(tr("Capture canceled", "已取消捕获").into());
        }
    });

    let weak = ui.as_weak();
    let state_for_save = Rc::clone(&state);
    let engine_for_save = engine.clone();
    ui.on_save_flexikey_editor(
        move |binding_index, device, key_code, key_name, action_type, value| {
            let state_snapshot = state_for_save.borrow();
            let Some(profile_position) = active_profile_position(&state_snapshot.store) else {
                return;
            };
            let Some(profile) = state_snapshot.store.profiles.get(profile_position) else {
                return;
            };
            let binding = match editor_binding(
                profile,
                binding_index,
                device,
                key_code.as_str(),
                key_name.as_str(),
                action_type,
                value.as_str(),
            ) {
                Ok(binding) => binding,
                Err(error) => {
                    if let Some(ui) = weak.upgrade() {
                        ui.set_flexikey_editor_status(error.into());
                    }
                    return;
                }
            };
            let mut next = state_snapshot.store.clone();
            drop(state_snapshot);
            let changed = if let Ok(index) = usize::try_from(binding_index) {
                next.replace_binding(profile_position, index, binding)
            } else {
                next.add_binding(profile_position, binding)
            };
            if !changed {
                if let Some(ui) = weak.upgrade() {
                    ui.set_flexikey_editor_status(
                        tr("The selected binding no longer exists", "所选绑定已不存在").into(),
                    );
                }
                return;
            }
            let report = match engine_for_save.update_profile(next.active()) {
                Ok(report) => report,
                Err(error) => {
                    if let Some(ui) = weak.upgrade() {
                        ui.set_flexikey_editor_status(
                            format!(
                                "{}: {error}",
                                tr("Action engine rejected the binding", "动作引擎拒绝了该绑定")
                            )
                            .into(),
                        );
                    }
                    return;
                }
            };
            let previous = state_for_save.borrow().store.clone();
            if let Err(error) = save_flexikey_store(&state_for_save.borrow().path, &next) {
                let _ = engine_for_save.update_profile(previous.active());
                let (engine_report, engine_error) =
                    sync_flexikey_engine(&engine_for_save, &previous);
                let mut state = state_for_save.borrow_mut();
                state.engine_report = engine_report;
                state.engine_error = engine_error;
                state.operation_error = Some(format!(
                    "{}: {error}",
                    tr("Could not save profile.xml", "无法保存 profile.xml")
                ));
                drop(state);
                if let Some(ui) = weak.upgrade() {
                    ui.set_flexikey_editor_status(error.into());
                    render_flexikey(&ui, &state_for_save.borrow());
                }
                return;
            }
            let mut state = state_for_save.borrow_mut();
            state.store = next;
            state.load_error = None;
            state.operation_error = None;
            state.operation_notice = Some(
                tr(
                    "Binding saved to profile.xml and loaded into the input engine.",
                    "绑定已保存到 profile.xml，并载入输入引擎。",
                )
                .to_owned(),
            );
            state.activated_index = None;
            state.engine_report = Some(report);
            state.engine_error = None;
            drop(state);
            if let Some(ui) = weak.upgrade() {
                ui.set_flexikey_capture_armed(false);
                ui.set_flexikey_editor_status("".into());
                ui.set_flexikey_editor_open(false);
                render_flexikey(&ui, &state_for_save.borrow());
            }
        },
    );

    let weak = ui.as_weak();
    let state_for_delete = Rc::clone(&state);
    let engine_for_delete = engine.clone();
    ui.on_delete_flexikey_binding(move |binding_index| {
        let Ok(binding_index) = usize::try_from(binding_index) else {
            return;
        };
        let state_snapshot = state_for_delete.borrow();
        let Some(profile_position) = active_profile_position(&state_snapshot.store) else {
            return;
        };
        let mut next = state_snapshot.store.clone();
        drop(state_snapshot);
        if !next.remove_binding(profile_position, binding_index) {
            return;
        }
        let report = match engine_for_delete.update_profile(next.active()) {
            Ok(report) => report,
            Err(error) => {
                if let Some(ui) = weak.upgrade() {
                    ui.set_flexikey_editor_status(
                        format!(
                            "{}: {error}",
                            tr("Action engine rejected the change", "动作引擎拒绝了更改")
                        )
                        .into(),
                    );
                }
                return;
            }
        };
        let previous = state_for_delete.borrow().store.clone();
        if let Err(error) = save_flexikey_store(&state_for_delete.borrow().path, &next) {
            let _ = engine_for_delete.update_profile(previous.active());
            let mut state = state_for_delete.borrow_mut();
            state.operation_error = Some(format!(
                "{}: {error}",
                tr("Could not save profile.xml", "无法保存 profile.xml")
            ));
            drop(state);
            if let Some(ui) = weak.upgrade() {
                ui.set_flexikey_editor_status(error.into());
                render_flexikey(&ui, &state_for_delete.borrow());
            }
            return;
        }
        let mut state = state_for_delete.borrow_mut();
        state.store = next;
        state.load_error = None;
        state.operation_error = None;
        state.operation_notice = Some(
            tr(
                "Binding removed from profile.xml and the input engine.",
                "绑定已从 profile.xml 和输入引擎中移除。",
            )
            .to_owned(),
        );
        state.activated_index = None;
        state.engine_report = Some(report);
        state.engine_error = None;
        drop(state);
        if let Some(ui) = weak.upgrade() {
            ui.set_flexikey_editor_open(false);
            ui.set_flexikey_editor_status("".into());
            render_flexikey(&ui, &state_for_delete.borrow());
        }
    });

    let weak = ui.as_weak();
    let state_for_reload = Rc::clone(&state);
    let engine_for_reload = engine.clone();
    ui.on_reload_flexikey(move || {
        engine_for_reload.cancel_capture();
        {
            let mut state = state_for_reload.borrow_mut();
            let mut next = load_flexikey_view(&state.path);
            let (report, error) = sync_flexikey_engine(&engine_for_reload, &next.store);
            next.engine_report = report;
            next.engine_error = error;
            *state = next;
        }
        if let Some(ui) = weak.upgrade() {
            ui.set_flexikey_capture_armed(false);
            ui.set_flexikey_editor_open(false);
            render_flexikey(&ui, &state_for_reload.borrow());
        }
    });

    state
}

fn flexikey_visible_profile(state: &FlexiKeyViewState) -> Option<&FlexiKeyProfile> {
    state.store.active()
}

fn flexikey_engine_status(state: &FlexiKeyViewState) -> String {
    if let Some(error) = &state.engine_error {
        return format!(
            "{}: {error}",
            tr("Action engine unavailable", "动作引擎不可用")
        );
    }
    let Some(report) = &state.engine_report else {
        return String::new();
    };
    format!(
        "{} · {} {} · {} {}",
        tr("Input hooks active", "输入钩子已启用"),
        report.keyboard_actions,
        tr("keyboard action(s)", "个键盘动作"),
        report.mouse_actions,
        tr("mouse action(s)", "个鼠标动作")
    )
}

pub(crate) fn render_flexikey(ui: &MainWindow, state: &FlexiKeyViewState) {
    let names = state
        .store
        .profiles
        .iter()
        .map(|profile| {
            slint::SharedString::from(format!("{:02}  {}", profile.index + 1, profile.name))
        })
        .collect::<Vec<_>>();
    ui.set_flexikey_profile_names(Rc::new(VecModel::from(names)).into());
    ui.set_flexikey_path(state.path.to_string_lossy().to_string().into());
    let visible_profile = flexikey_visible_profile(state);
    ui.set_flexikey_active_index(
        visible_profile
            .map(|profile| profile.index as i32)
            .unwrap_or(-1),
    );
    ui.set_flexikey_engine_status(flexikey_engine_status(state).into());

    if let Some(profile) = visible_profile {
        let bindings = profile
            .bindings
            .iter()
            .enumerate()
            .map(|(index, binding)| format_binding_label(index, binding).into())
            .collect::<Vec<slint::SharedString>>();
        ui.set_flexikey_binding_labels(Rc::new(VecModel::from(bindings)).into());
        ui.set_flexikey_keyboard_enabled(profile.keyboard_enabled);
        ui.set_flexikey_mouse_enabled(profile.mouse_enabled);
        ui.set_flexikey_profile_name(profile.name.clone().into());
        ui.set_flexikey_active_title(profile.name.clone().into());
        ui.set_flexikey_summary(format_flexikey_profile(profile).into());
    } else {
        ui.set_flexikey_binding_labels(
            Rc::new(VecModel::from(Vec::<slint::SharedString>::new())).into(),
        );
        ui.set_flexikey_keyboard_enabled(false);
        ui.set_flexikey_mouse_enabled(false);
        ui.set_flexikey_profile_name("".into());
        ui.set_flexikey_active_title("FlexiKey".into());
        ui.set_flexikey_summary(
            tr("No profiles are available.", "当前没有可用的配置文件。").into(),
        );
    }

    let status = if let Some(error) = &state.operation_error {
        error.clone()
    } else if let Some(notice) = &state.operation_notice {
        notice.clone()
    } else if let Some(profile) = state.activated_index.and_then(|index| {
        state
            .store
            .profiles
            .iter()
            .find(|profile| profile.index == index)
    }) {
        format!(
            "{} {} · {}",
            tr("Activated and saved", "已启用并保存："),
            profile.name,
            tr("action engine reloaded", "动作引擎已重新加载")
        )
    } else if let Some(error) = &state.load_error {
        format!(
            "{} {error}",
            tr("Profile XML unavailable:", "配置 XML 不可用：")
        )
    } else {
        let count = state.store.profiles.len();
        format!(
            "{} {count} {}",
            tr("Loaded", "已载入"),
            tr("profile(s) from CC4.0 XML", "个来自 CC4.0 XML 的配置文件")
        )
    };
    ui.set_flexikey_status(status.into());
}

fn format_flexikey_profile(profile: &FlexiKeyProfile) -> String {
    format!(
        "{} {} · {} {} · {} {}\n{} {}",
        profile.bindings.len(),
        tr("binding(s)", "条绑定"),
        tr("keyboard", "键盘"),
        if profile.keyboard_enabled {
            tr("enabled", "启用")
        } else {
            tr("disabled", "禁用")
        },
        tr("mouse", "鼠标"),
        if profile.mouse_enabled {
            tr("enabled", "启用")
        } else {
            tr("disabled", "禁用")
        },
        tr("Device mode:", "设备模式："),
        profile.current_device
    )
}

fn save_flexikey_store(path: &Path, store: &FlexiKeyStore) -> Result<(), String> {
    let started = std::time::Instant::now();
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    let temporary = path.with_extension("xml.neo.tmp");
    std::fs::write(&temporary, store.to_xml()).map_err(|error| {
        format!(
            "could not write temporary profile file {}: {error}",
            temporary.display()
        )
    })?;

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt as _;
        use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MoveFileExW};
        let source = temporary
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let destination = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        // SAFETY: both paths are live NUL-terminated UTF-16 buffers, and the
        // source file remains available until this synchronous move returns.
        let moved = unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING,
            )
        };
        if moved == 0 {
            let error = std::io::Error::last_os_error();
            let _ = std::fs::remove_file(&temporary);
            return Err(format!("could not replace {}: {error}", path.display()));
        }
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(&temporary, path)
            .map_err(|error| format!("could not replace {}: {error}", path.display()))?;
    }
    log_action(&format!(
        "flexikey: saved {} in {}us",
        path.display(),
        started.elapsed().as_micros()
    ));
    Ok(())
}

fn commit_flexikey_store(
    state: &Rc<RefCell<FlexiKeyViewState>>,
    engine: &FlexiKeyEngine,
    next: FlexiKeyStore,
    notice: String,
) -> Result<(), String> {
    let previous = state.borrow().store.clone();
    let path = state.borrow().path.clone();
    let report = engine.update_profile(next.active())?;
    if let Err(error) = save_flexikey_store(&path, &next) {
        let _ = engine.update_profile(previous.active());
        let (engine_report, engine_error) = sync_flexikey_engine(engine, &previous);
        let mut state = state.borrow_mut();
        state.engine_report = engine_report;
        state.engine_error = engine_error;
        return Err(error);
    }
    let mut state = state.borrow_mut();
    state.store = next;
    state.load_error = None;
    state.operation_error = None;
    state.operation_notice = Some(notice);
    state.activated_index = None;
    state.engine_report = Some(report);
    state.engine_error = None;
    Ok(())
}

#[cfg(test)]
mod flexikey_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn temp_profile_path(label: &str) -> PathBuf {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "neo-flexikey-{label}-{}-{nanos}-{sequence}.xml",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn selected_profile_is_saved_and_reloaded() {
        let path = temp_profile_path("save-test");
        let xml = r#"<root>
            <Profile_0 Name="Gaming" IsSelected="1"/>
            <Profile_1 Name="Work" IsSelected="0"/>
        </root>"#;
        let mut store = FlexiKeyStore::from_xml(xml).expect("initial store");
        assert!(store.select(1));
        save_flexikey_store(&path, &store).expect("profile save");
        assert!(!path.with_extension("xml.neo.tmp").exists());
        let saved = std::fs::read_to_string(&path).expect("saved XML");
        let loaded = FlexiKeyStore::from_xml(&saved).expect("saved XML must parse");
        assert_eq!(
            loaded.active().map(|profile| profile.name.as_str()),
            Some("Work")
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn editor_trigger_codes_accept_decimal_and_hex() {
        assert_eq!(parse_trigger_key_code("0xE053"), Ok(0xE053));
        assert_eq!(parse_trigger_key_code("64"), Ok(64));
        assert!(parse_trigger_key_code("").is_err());
        assert!(parse_trigger_key_code("not-a-key").is_err());
    }

    #[test]
    fn editing_launch_preserves_vendor_event_id() {
        let existing = FlexiKeyAction::Launch {
            app_path: r"C:\old.exe".to_owned(),
            event_id: 42,
        };
        let updated = editor_action(Some(&existing), 2, r"C:\new.exe").expect("launch update");
        assert_eq!(
            updated,
            FlexiKeyAction::Launch {
                app_path: r"C:\new.exe".to_owned(),
                event_id: 42,
            }
        );
    }

    #[test]
    fn editing_text_preserves_start_and_send_keys() {
        let step = |key_code| neo_control_protocol::MacroStep {
            key_code,
            kind: neo_control_protocol::MacroStepKind::Make,
            delay_ms: 5,
            group: 0,
            display: String::new(),
        };
        let existing = FlexiKeyAction::Text {
            start_keys: vec![step(1)],
            text: "old".to_owned(),
            send_keys: vec![step(2)],
        };
        let updated = editor_action(Some(&existing), 3, "new text").expect("text update");
        assert_eq!(
            updated,
            FlexiKeyAction::Text {
                start_keys: vec![step(1)],
                text: "new text".to_owned(),
                send_keys: vec![step(2)],
            }
        );
    }

    #[test]
    fn mouse_binding_rejects_vendor_unknown_button() {
        let profile = FlexiKeyProfile {
            index: 0,
            name: "Mouse".to_owned(),
            icon: String::new(),
            current_device: 2,
            keyboard_enabled: true,
            mouse_enabled: true,
            selected: true,
            bindings: Vec::new(),
        };
        let error = editor_binding(&profile, -1, 2, "0x20", "Side", 4, "")
            .expect_err("unknown mouse button must fail");
        assert!(error.contains("鼠标触发") || error.contains("Mouse triggers"));
    }

    #[test]
    fn missing_profile_offers_a_blank_active_profile() {
        let path = temp_profile_path("missing");
        let state = load_flexikey_view(&path);
        let profile = state.store.active().expect("blank active profile");
        assert!(profile.bindings.is_empty());
        assert!(profile.keyboard_enabled);
        assert!(profile.mouse_enabled);
        assert!(state.load_error.is_none());
        assert!(state.operation_notice.is_some());
    }
}

fn main() {
    println!("cargo:rerun-if-changed=ui");
    println!("cargo:rerun-if-changed=../../assets/ram_control_center_logo.svg");
    println!("cargo:rerun-if-changed=ui/translations");

    compile_runtime_catalog();

    let config = slint_build::CompilerConfiguration::new()
        .with_bundled_translations("ui/translations")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/main.slint", config).expect("failed to compile Slint UI");
}

fn compile_runtime_catalog() {
    let path = "ui/translations/zh-CN/LC_MESSAGES/neo-control.po";
    let source = std::fs::read_to_string(path).expect("failed to read Chinese translation catalog");
    let mut entries = Vec::new();
    let mut msgid = None;
    let mut msgstr = None;
    let mut active_field = None;

    let flush = |entries: &mut Vec<(String, String)>,
                 msgid: &mut Option<String>,
                 msgstr: &mut Option<String>| {
        if let (Some(id), Some(value)) = (msgid.take(), msgstr.take())
            && !id.is_empty()
            && !value.is_empty()
        {
            entries.push((id, value));
        }
    };

    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() {
            flush(&mut entries, &mut msgid, &mut msgstr);
            active_field = None;
        } else if let Some(value) = line.strip_prefix("msgid ") {
            flush(&mut entries, &mut msgid, &mut msgstr);
            msgid = Some(decode_po_string(value));
            active_field = Some("msgid");
        } else if let Some(value) = line.strip_prefix("msgstr ") {
            msgstr = Some(decode_po_string(value));
            active_field = Some("msgstr");
        } else if line.starts_with('"') {
            let value = decode_po_string(line);
            match active_field {
                Some("msgid") => {
                    msgid.get_or_insert_with(String::new).push_str(&value);
                }
                Some("msgstr") => {
                    msgstr.get_or_insert_with(String::new).push_str(&value);
                }
                _ => {}
            }
        }
    }
    flush(&mut entries, &mut msgid, &mut msgstr);

    let mut generated = String::from("pub(crate) const ZH_CATALOG: &[(&str, &str)] = &[\n");
    entries.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    for (id, value) in entries {
        generated.push_str(&format!("    ({id:?}, {value:?}),\n"));
    }
    generated.push_str("];\n");
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR missing"))
        .join("runtime_translations.rs");
    std::fs::write(output, generated).expect("failed to write runtime translation catalog");
}

fn decode_po_string(value: &str) -> String {
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .expect("invalid quoted PO string");
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        match chars.next().expect("trailing escape in PO string") {
            'n' => output.push('\n'),
            'r' => output.push('\r'),
            't' => output.push('\t'),
            '"' => output.push('"'),
            '\\' => output.push('\\'),
            other => panic!("unsupported PO escape: \\{other}"),
        }
    }
    output
}

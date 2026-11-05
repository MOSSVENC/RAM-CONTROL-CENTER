fn main() {
    println!("cargo:rerun-if-changed=ui");
    println!("cargo:rerun-if-changed=../../assets/neo_control_logo.svg");
    println!("cargo:rerun-if-changed=ui/translations");
    let config = slint_build::CompilerConfiguration::new()
        .with_bundled_translations("ui/translations")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/main.slint", config).expect("failed to compile Slint UI");
}

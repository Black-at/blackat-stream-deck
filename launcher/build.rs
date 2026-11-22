fn main() {
    // Style sombre pour les champs de saisie (mot de passe), assorti au fond nuit.
    let config = slint_build::CompilerConfiguration::new().with_style("fluent-dark".into());
    slint_build::compile_with_config("ui/launcher.slint", config).expect("compilation de l'interface Slint");
}

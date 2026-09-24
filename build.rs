fn main() {
    println!("cargo:rerun-if-changed=res/usados.rc");
    println!("cargo:rerun-if-changed=res/usados.exe.manifest");
    println!("cargo:rerun-if-changed=res/usados.ico");

    // Ícone, manifesto (visual moderno, sem admin) e informações de versão do .exe
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("res/usados.rc", embed_resource::NONE)
            .manifest_required()
            .unwrap();
    }
}

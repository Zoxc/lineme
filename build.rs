fn main() {
    println!("cargo:rerun-if-changed=assets/lineme.rc");
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-changed=assets/file.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/lineme.rc", embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}

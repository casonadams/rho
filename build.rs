fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let icon_path = std::path::Path::new(&manifest_dir).join("assets").join("rho.ico");
    println!("cargo:rerun-if-changed={}", icon_path.display());

    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon(icon_path.to_str().unwrap_or("assets/rho.ico"));
        let _ = res.compile();
    }
}

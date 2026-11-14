fn main() {
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=assets/arthos.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/arthos.ico");
        res.set("ProductName", "Arthos");
        res.set("FileDescription", "Arthos — AI-Native Lightweight Web Browser");
        if let Err(e) = res.compile() {
            println!("cargo:warning=Failed to embed Windows icon resource: {e}");
        }
    }
}

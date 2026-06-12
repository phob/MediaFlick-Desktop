#[cfg(target_os = "windows")]
fn main() {
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("resources/win/jellyfin.ico");
    resource.set("CompanyName", "Jellyfin");
    resource.set("FileDescription", "Jellyfin MPV");
    resource.set("InternalName", "jellyfin-mpv");
    resource.set("OriginalFilename", "jellyfin-mpv.exe");
    resource.set("ProductName", "Jellyfin MPV");
    resource
        .compile()
        .expect("failed to compile Windows resources");
}

#[cfg(not(target_os = "windows"))]
fn main() {}

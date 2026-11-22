fn main() {
    slint_build::compile("ui/app.slint").expect("slint build failed");

    // Іконка для .exe на Windows.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        // Офіційна збірка — бірюзова іконка, Altherra Offline — червона.
        let official = std::env::var_os("CARGO_FEATURE_MICROSOFT").is_some();
        res.set_icon(if official { "assets/icon.ico" } else { "assets/icon-offline.ico" });
        res.set("ProductName", if official { "Altherra" } else { "Altherra Offline" });
        res.set("FileDescription", if official { "Altherra Launcher" } else { "Altherra Offline" });
        res.compile().expect("windows resources");
    }
}

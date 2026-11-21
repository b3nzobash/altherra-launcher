fn main() {
    slint_build::compile("ui/app.slint").expect("slint build failed");

    // Іконка для .exe на Windows.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "Altherra");
        res.set("FileDescription", "Altherra Launcher");
        res.compile().expect("windows resources");
    }
}

// Build script de Karkinos: su único trabajo es embeber el ícono de la app dentro
// del ejecutable de Windows. En el resto de las plataformas no compila recursos,
// solo le avisa a cargo cuándo tiene que volver a correr.

fn main() {
    // Si el .ico cambia hay que volver a embeberlo, así que lo declaramos como
    // entrada del build script.
    println!("cargo:rerun-if-changed=assets/karkinos.ico");

    // WindowsResource agrega el ícono como recurso del .exe: es el que se ve en el
    // Explorador de archivos, en la barra de tareas y en los accesos directos.
    // Miramos el TARGET (CARGO_CFG_TARGET_OS) y no el host, porque el recurso solo
    // existe en Windows: en Linux/macOS el build script no tiene nada que hacer.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/karkinos.ico")
            .compile()
            .expect("no se pudo embeber el ícono de Karkinos en el ejecutable de Windows");
    }
}

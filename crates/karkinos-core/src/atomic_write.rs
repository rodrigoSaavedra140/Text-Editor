//! Escritura atómica de archivos (S1).
//!
//! El destino nunca se trunca: los bytes van primero a un temporal hermano
//! (mismo filesystem, para que el `rename` sea atómico) y recién cuando están
//! completos y sincronizados se reemplaza el archivo del usuario.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Nombre del temporal hermano del destino. Va oculto y en el mismo
/// directorio para que el `rename` final no cruce filesystems.
fn temp_path(path: &Path) -> io::Result<PathBuf> {
    let file_name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "atomic write target has no file name",
        )
    })?;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    Ok(dir.join(format!(".{file_name}.karkinos-tmp")))
}

/// Escribe `data` en `path` de forma atómica: primero a un archivo
/// temporal hermano, `sync_all`, y después `rename` sobre el destino.
/// `on_progress(written)` se llama después de cada pedazo escrito.
pub fn write_atomic(
    path: &Path,
    data: &[u8],
    mut on_progress: impl FnMut(usize),
) -> io::Result<()> {
    let tmp_path = temp_path(path)?;
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
    let old_path = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(".{file_name}.karkinos-tmp.old"));

    // 1) Escribimos el temporal completo, en pedazos parejos (mínimo
    // un byte por paso) para que la barra de progreso tenga algo real
    // que mostrar incluso con archivos chicos.
    let write_result = (|| -> io::Result<()> {
        let mut file = fs::File::create(&tmp_path)?;
        let total = data.len();
        let steps = 20usize;
        let chunk_size = (total / steps).max(1);
        let mut written = 0usize;
        while written < total {
            let end = (written + chunk_size).min(total);
            file.write_all(&data[written..end])?;
            written = end;
            on_progress(written);
        }
        file.sync_all()?;
        Ok(())
    })();
    if let Err(err) = write_result {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }

    // 2) Conservamos los permisos del destino (p. ej. el bit de
    // ejecución de un script) antes de reemplazarlo.
    if path.exists() {
        let result = fs::metadata(path)
            .and_then(|metadata| fs::set_permissions(&tmp_path, metadata.permissions()));
        if let Err(err) = result {
            let _ = fs::remove_file(&tmp_path);
            return Err(err);
        }
    }

    // 3) Reemplazo atómico. Si el rename falla (Windows no pisa
    // archivos existentes) movemos el destino a `.old`, renombramos el
    // temporal y borramos `.old`; si el segundo rename falla,
    // restauramos el original.
    if let Err(err) = fs::rename(&tmp_path, path) {
        if path.exists() {
            let _ = fs::remove_file(&old_path);
            if let Err(backup_err) = fs::rename(path, &old_path) {
                let _ = fs::remove_file(&tmp_path);
                return Err(backup_err);
            }
            if let Err(rename_err) = fs::rename(&tmp_path, path) {
                let _ = fs::rename(&old_path, path);
                let _ = fs::remove_file(&tmp_path);
                return Err(rename_err);
            }
            let _ = fs::remove_file(&old_path);
        } else {
            let _ = fs::remove_file(&tmp_path);
            return Err(err);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "karkinos_atomic_write_{}-{id}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn write_atomic_crea_archivo_nuevo() {
        let dir = TestDir::new();
        let target = dir.path().join("nuevo.txt");

        write_atomic(&target, b"hola", |_| {}).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"hola");
    }

    #[test]
    fn write_atomic_reemplaza_contenido_existente() {
        let dir = TestDir::new();
        let target = dir.path().join("existente.txt");
        fs::write(&target, "viejo").unwrap();

        write_atomic(&target, b"nuevo", |_| {}).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "nuevo");
    }

    #[test]
    fn write_atomic_no_deja_temporales() {
        let dir = TestDir::new();
        let target = dir.path().join("destino.txt");

        write_atomic(&target, b"contenido", |_| {}).unwrap();

        let entries: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries, vec!["destino.txt".to_string()]);
    }

    #[test]
    fn write_atomic_fallido_preserva_original() {
        let dir = TestDir::new();
        // El "original" es un archivo que ocupa el lugar del directorio padre:
        // crear el temporal adentro falla y el archivo debe quedar intacto.
        let blocker = dir.path().join("blocker");
        fs::write(&blocker, "original").unwrap();
        let target = blocker.join("destino.txt");

        assert!(write_atomic(&target, b"nuevo", |_| {}).is_err());

        assert_eq!(fs::read_to_string(&blocker).unwrap(), "original");
        let entries: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries, vec!["blocker".to_string()]);
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_preserva_permisos() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TestDir::new();
        let target = dir.path().join("script.sh");
        fs::write(&target, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();

        write_atomic(&target, b"#!/bin/sh\necho hola\n", |_| {}).unwrap();

        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
    }

    #[test]
    fn write_atomic_reporta_progreso() {
        let dir = TestDir::new();
        let target = dir.path().join("progreso.txt");
        let data = vec![b'x'; 10_000];
        let mut valores = Vec::new();

        write_atomic(&target, &data, |written| valores.push(written)).unwrap();

        assert_eq!(valores.last().copied(), Some(data.len()));
        assert!(valores.windows(2).all(|par| par[0] <= par[1]));
    }
}

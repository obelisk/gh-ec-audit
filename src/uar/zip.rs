use hex;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use walkdir::WalkDir;
use zip::write::FileOptions;

/// Generate a zip archive with the content of a given folder.
/// On success, return the SHA256 hash of the generated zip archive.
pub fn zip_dir_recursive<P: AsRef<Path>>(src_dir: P, zip_path: P) -> std::io::Result<String> {
    let src_dir = src_dir.as_ref();
    let zip_file = File::create(&zip_path)?;
    let mut zip = zip::ZipWriter::new(zip_file);

    let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    for entry in WalkDir::new(src_dir) {
        let entry = entry?;
        let path = entry.path();
        let rel_path = path.strip_prefix(src_dir).unwrap();

        if path.is_file() {
            let mut f = File::open(path)?;
            let mut buffer = Vec::new();
            f.read_to_end(&mut buffer)?;

            // Convert path separators to "/" for zip format
            let zip_path = rel_path.to_string_lossy().replace("\\", "/");
            zip.start_file(zip_path, options)?;
            zip.write_all(&buffer)?;
        } else if rel_path.as_os_str().len() != 0 {
            // Create directories in the zip if not the root
            let dir_name = rel_path.to_string_lossy().replace("\\", "/") + "/";
            zip.add_directory(dir_name, options)?;
        }
    }

    zip.finish()?; // finalize the archive

    // Reopen ZIP to calculate SHA256
    let mut file = File::open(&zip_path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    let mut hasher = Sha256::new();
    hasher.update(&buffer);
    let hash = hasher.finalize();

    Ok(hex::encode(hash))
}

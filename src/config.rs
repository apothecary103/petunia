use std::path::PathBuf;

pub fn dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("petunia")
}

pub fn store_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("petunia")
        .join("petunia.db3")
}

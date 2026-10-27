use anyhow::{Context, Result};
use std::env;
use std::io;
use std::path::Path;
use tokio::fs::{self};
use tokio::io::AsyncWriteExt;

use super::Config;

/// Load the app configuration, or create a default one if missing.
pub async fn get_app_config() -> Result<Config> {
    const FILENAME: &str = "config.toml";
    const PKG_NAME: &str = env!("CARGO_PKG_NAME");

    let file_path = dirs::config_dir()
        .context("Unable to locate user config directory")?
        .join(PKG_NAME)
        .join(FILENAME);

    if create_default_config(&file_path).await? {
        println!("Created default config at {:?}", file_path);
    }

    let contents = fs::read_to_string(file_path).await?;

    toml::from_str(&contents).context("Failed to parse TOML config")
}

/// Write the default config to `file_path` if no file exists there yet.
/// Returns `true` when the file was created.
///
/// The config holds SMTP credentials, so on Unix the file is created as 0600
/// and a missing parent directory as 0700. `create_new` makes the existence
/// check and the creation one atomic step, so concurrent runs cannot both
/// write the file.
async fn create_default_config(file_path: &Path) -> Result<bool> {
    let dir = file_path
        .parent()
        .context("Config path has no parent directory")?;

    let mut dir_builder = fs::DirBuilder::new();
    dir_builder.recursive(true);
    #[cfg(unix)]
    dir_builder.mode(0o700);
    dir_builder.create(dir).await?;

    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);

    let mut file = match options.open(file_path).await {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => return Ok(false),
        Err(err) => return Err(err.into()),
    };

    file.write_all(super::constants::CONFIG_FILE_CONTENT.as_bytes())
        .await?;
    file.flush().await?;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A config path inside a fresh temporary directory unique to `name`
    async fn temp_config_path(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("hark-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir).await;
        dir.join("config.toml")
    }

    #[tokio::test]
    async fn creates_default_config_when_missing() {
        let file_path = temp_config_path("missing").await;

        assert!(create_default_config(&file_path).await.unwrap());
        assert_eq!(
            fs::read_to_string(&file_path).await.unwrap(),
            super::super::constants::CONFIG_FILE_CONTENT
        );

        fs::remove_dir_all(file_path.parent().unwrap())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn keeps_existing_config() {
        let file_path = temp_config_path("existing").await;
        fs::create_dir_all(file_path.parent().unwrap())
            .await
            .unwrap();
        fs::write(&file_path, "user config").await.unwrap();

        assert!(!create_default_config(&file_path).await.unwrap());
        assert_eq!(fs::read_to_string(&file_path).await.unwrap(), "user config");

        fs::remove_dir_all(file_path.parent().unwrap())
            .await
            .unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn creates_config_readable_only_by_owner() {
        use std::os::unix::fs::PermissionsExt;

        let file_path = temp_config_path("permissions").await;
        let dir = file_path.parent().unwrap();

        create_default_config(&file_path).await.unwrap();

        let file_mode = fs::metadata(&file_path).await.unwrap().permissions().mode();
        let dir_mode = fs::metadata(dir).await.unwrap().permissions().mode();
        assert_eq!(
            format!("{:o}", file_mode & 0o777),
            "600",
            "config file mode"
        );
        assert_eq!(format!("{:o}", dir_mode & 0o777), "700", "config dir mode");

        fs::remove_dir_all(dir).await.unwrap();
    }
}

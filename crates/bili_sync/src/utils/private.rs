use super::{Error, Result};
#[cfg(windows)]
use base64::Engine;
use serde::Serialize;
use std::path::Path;

pub fn private_dir(root: &Path) -> Result<()> {
    std::fs::create_dir_all(root).map_err(Error::from)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)).map_err(|_| Error::File)?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let account = format!(
            "{}\\{}:(OI)(CI)F",
            std::env::var("USERDOMAIN").map_err(|_| Error::File)?,
            std::env::var("USERNAME").map_err(|_| Error::File)?
        );
        let result = std::process::Command::new("icacls")
            .arg(root)
            .args(["/inheritance:r", "/grant:r", &account])
            .creation_flags(0x08000000)
            .output()
            .map_err(|_| Error::File)?;
        if !result.status.success() {
            return Err(Error::File);
        }
        // icacls preserves other explicit ACEs; reject shared directories before any secret write.
        let check = "$ErrorActionPreference='Stop'; $a=[System.IO.Directory]::GetAccessControl($env:FAVKEEP_PRIVATE_DIR); $me=[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value; if(!$a.AreAccessRulesProtected){exit 1}; foreach($r in $a.GetAccessRules($true,$true,[System.Security.Principal.SecurityIdentifier])){if($r.AccessControlType -eq 'Allow' -and $r.IdentityReference.Value -notin @($me,'S-1-5-18','S-1-5-32-544')){exit 1}}";
        let bytes: Vec<u8> = check.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        let checked = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
            .env("FAVKEEP_PRIVATE_DIR", root)
            .creation_flags(0x08000000)
            .output()
            .map_err(|_| Error::File)?;
        if !checked.status.success() {
            return Err(Error::File);
        }
    }
    Ok(())
}

pub fn write_private(path: &Path, data: &impl Serialize) -> Result<()> {
    use std::io::Write;
    let parent = path.parent().ok_or(Error::File)?;
    private_dir(parent)?;
    let bytes = serde_json::to_vec_pretty(data).map_err(|_| Error::InvalidData)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(Error::from)?;
    file.write_all(&bytes).map_err(Error::from)?;
    file.flush().map_err(Error::from)?;
    file.as_file().sync_all().map_err(Error::from)?;
    file.persist(path).map_err(|e| Error::from(e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn private_storage_restricts_directory_and_export() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("private");
        private_dir(&root).unwrap();
        write_private(&root.join("auth.json"), &json!({"synthetic":"fixture"})).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&root).unwrap().permissions().mode() & 0o777, 0o700);
            assert_eq!(
                std::fs::metadata(root.join("auth.json")).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        #[cfg(windows)]
        {
            let result = std::process::Command::new("icacls").arg(&root).output().unwrap();
            let text = String::from_utf8_lossy(&result.stdout);
            assert!(result.status.success());
            assert!(
                !text.contains("(I)"),
                "private directory must not inherit parent access"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn existing_explicit_foreign_access_is_rejected_before_writing_secrets() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("private");
        private_dir(&root).unwrap();
        let result = std::process::Command::new("icacls")
            .arg(&root)
            .args(["/grant", "*S-1-1-0:(OI)(CI)R"])
            .output()
            .unwrap();
        assert!(result.status.success());
        assert_eq!(private_dir(&root), Err(Error::File));
        assert!(!root.join("auth.json").exists());
    }
}

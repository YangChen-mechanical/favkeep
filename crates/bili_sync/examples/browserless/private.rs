use anyhow::{Context, Result, ensure};
#[cfg(windows)]
use base64::Engine;
use std::path::Path;

pub fn private_dir(path: &Path) -> Result<()> {
    ensure!(path.is_absolute(), "output directory must be absolute");
    // create_dir prevents using an existing application, probe, symlink or credential directory.
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(path)
            .context("new private output directory required")?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::fs::create_dir(path).context("new private output directory required")?;
        let account = format!(
            "{}\\{}:(OI)(CI)F",
            std::env::var("USERDOMAIN")?,
            std::env::var("USERNAME")?
        );
        let status = std::process::Command::new("icacls")
            .arg(path)
            .args(["/inheritance:r", "/grant:r", &account])
            .creation_flags(0x08000000)
            .output()?;
        ensure!(status.status.success(), "private ACL setup failed");
        let check = "$ErrorActionPreference='Stop'; $a=[System.IO.Directory]::GetAccessControl($env:FAVKEEP_PRIVATE_DIR); $me=[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value; if(!$a.AreAccessRulesProtected){exit 1}; foreach($r in $a.GetAccessRules($true,$true,[System.Security.Principal.SecurityIdentifier])){if($r.AccessControlType -eq 'Allow' -and $r.IdentityReference.Value -notin @($me,'S-1-5-18','S-1-5-32-544')){exit 1}}";
        let encoded = base64::engine::general_purpose::STANDARD
            .encode(check.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>());
        let result = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
            .env("FAVKEEP_PRIVATE_DIR", path)
            .creation_flags(0x08000000)
            .output()?;
        ensure!(result.status.success(), "private ACL verification failed");
    }
    Ok(())
}

pub fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

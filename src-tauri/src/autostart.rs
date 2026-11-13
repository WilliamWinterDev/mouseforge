//! "Start with Windows" as a Task Scheduler logon task.
//!
//! A HKCU\...\Run entry can silently go missing: a process running inside a
//! packaged (MSIX) app's sandbox, such as a terminal built into a Store app, has
//! its HKCU writes redirected to a private hive that Explorer never reads at
//! login. A task is registered through the Task Scheduler service, so it always
//! lands, and a task for your own account doesn't need admin rights.

use std::os::windows::process::CommandExt;
use std::process::{Command, Output};

const TASK: &str = "MouseForge";
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn run(program: &str, args: &[&str]) -> std::io::Result<Output> {
    Command::new(program).args(args).creation_flags(CREATE_NO_WINDOW).output()
}

pub fn is_enabled() -> bool {
    run("schtasks", &["/Query", "/TN", TASK]).map_or(false, |o| o.status.success())
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    remove_run_entry();
    if enabled { create_task() } else { delete_task() }
}

/// Moves an older install's Run entry over to the logon task.
pub fn migrate() {
    let has_run_entry = run("reg", &["query", RUN_KEY, "/v", TASK]).map_or(false, |o| o.status.success());
    if has_run_entry {
        match set_enabled(true) {
            Ok(()) => crate::applog::log("autostart: moved from the Run key to a logon task"),
            Err(e) => crate::applog::log(format!("autostart: couldn't create the logon task: {e}")),
        }
    }
}

fn remove_run_entry() {
    let _ = run("reg", &["delete", RUN_KEY, "/v", TASK, "/f"]);
}

fn delete_task() -> Result<(), String> {
    if !is_enabled() {
        return Ok(());
    }
    check(run("schtasks", &["/Delete", "/TN", TASK, "/F"]))
}

fn create_task() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let user = match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
        (Ok(d), Ok(u)) => format!("{d}\\{u}"),
        (_, Ok(u)) => u,
        _ => return Err("can't tell which user is logged in".into()),
    };
    let xml = task_xml(&exe.to_string_lossy(), &user);

    // schtasks reads UTF-16 XML most reliably.
    let path = std::env::temp_dir().join("mouseforge-task.xml");
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let result = check(run("schtasks", &["/Create", "/TN", TASK, "/XML", &path.to_string_lossy(), "/F"]));
    let _ = std::fs::remove_file(&path);
    result
}

fn check(out: std::io::Result<Output>) -> Result<(), String> {
    let out = out.map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Normal priority matters: tasks default to below-normal (7), which would make
/// the input hooks lag when the PC is busy.
fn task_xml(exe: &str, user: &str) -> String {
    let (exe, user) = (escape(exe), escape(user));
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>Starts MouseForge in the tray when you log in.</Description></RegistrationInfo>
  <Triggers>
    <LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
    <StartWhenAvailable>false</StartWhenAvailable>
    <AllowHardTerminate>true</AllowHardTerminate>
  </Settings>
  <Actions Context="Author">
    <Exec><Command>"{exe}"</Command><Arguments>--minimized</Arguments></Exec>
  </Actions>
</Task>
"#
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn xml_escapes_paths() {
        let xml = super::task_xml(r"C:\A & B\MouseForge.exe", r"PC\Me");
        assert!(xml.contains(r#"<Command>"C:\A &amp; B\MouseForge.exe"</Command>"#));
        assert!(xml.contains(r"<UserId>PC\Me</UserId>"));
    }
}

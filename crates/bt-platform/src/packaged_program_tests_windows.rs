//! The package a program belongs to, asked of Windows (`packaged_program`'s Windows arm).

use super::windows_impl::appexeclink_family;
use super::*;
use std::path::{Path, PathBuf};
use windows::Win32::System::SystemServices::IO_REPARSE_TAG_APPEXECLINK;

/// The Store's PowerShell 7 alias on this machine and the package path it starts — only when the
/// alias really is an app execution alias. `None` on a machine with no Store install (a CI runner),
/// which the tests below say on stderr rather than pass in silence.
fn store_pwsh() -> Option<(PathBuf, PathBuf)> {
    let alias = Path::new(&std::env::var_os("LocalAppData")?)
        .join("Microsoft")
        .join("WindowsApps")
        .join("pwsh.exe");
    let data = super::windows_impl::reparse_data(&alias)?;
    appexeclink_family(&data)?;
    let strings = alias_strings(&data)?;
    Some((alias, PathBuf::from(strings.get(2)?)))
}

fn skipped(test: &str) {
    eprintln!(
        "packaged program test skipped={test} reason=no-store-appexeclink \
         (%LocalAppData%\\Microsoft\\WindowsApps\\pwsh.exe is not an app execution alias here)"
    );
}

/// Every NUL-terminated string in an alias's data after its version word.
fn alias_strings(buffer: &[u8]) -> Option<Vec<String>> {
    let length = usize::from(u16::from_le_bytes(buffer.get(4..6)?.try_into().ok()?));
    let data = buffer.get(12..8 + length)?;
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    Some(
        units
            .split(|unit| *unit == 0)
            .filter(|run| !run.is_empty())
            .map(String::from_utf16_lossy)
            .collect(),
    )
}

/// A reparse point's bytes: `tag`, then data of `version` and `strings`, each NUL-terminated.
fn reparse_buffer(tag: u32, version: u32, strings: &[&str]) -> Vec<u8> {
    let mut data = version.to_le_bytes().to_vec();
    for string in strings {
        for unit in string.encode_utf16().chain(std::iter::once(0)) {
            data.extend_from_slice(&unit.to_le_bytes());
        }
    }
    let mut buffer = tag.to_le_bytes().to_vec();
    buffer.extend_from_slice(&u16::try_from(data.len()).expect("small").to_le_bytes());
    buffer.extend_from_slice(&[0, 0]);
    buffer.extend_from_slice(&data);
    buffer
}

/// RED (T-CHECK-NEVER-OVERLAPS-A-PACKAGED-BIRTH) — **an app execution alias's data names the
/// family it starts**: the first string after the version word, whatever the target's path says
/// (here a path with a Chinese folder in it, which is not where the family is read from).
///
/// MUTATION: read the third string (the target path) instead of the first, or skip the version
/// check — the family is wrong, and the version-2 buffer is answered.
#[test]
fn an_alias_s_data_names_its_family() {
    let alias = reparse_buffer(
        IO_REPARSE_TAG_APPEXECLINK,
        3,
        &[
            "Microsoft.PowerShell_8wekyb3d8bbwe",
            "Microsoft.PowerShell_8wekyb3d8bbwe!App",
            r"D:\应用\WindowsApps\Microsoft.PowerShell_7.6.6.0_x64__8wekyb3d8bbwe\pwsh.exe",
            "0",
        ],
    );
    assert_eq!(
        appexeclink_family(&alias),
        Some(PackageFamily::named("microsoft.powershell_8wekyb3d8bbwe"))
    );
    let older = reparse_buffer(
        IO_REPARSE_TAG_APPEXECLINK,
        2,
        &["Microsoft.PowerShell_8wekyb3d8bbwe"],
    );
    assert_eq!(appexeclink_family(&older), None, "an unknown layout");
    // A symbolic link's tag, with data that would read as a family.
    let link = reparse_buffer(0xA000_000C, 3, &["Microsoft.PowerShell_8wekyb3d8bbwe"]);
    assert_eq!(appexeclink_family(&link), None, "not an alias");
}

/// RED — **the Store's alias and the package's own `pwsh.exe` are one package** on a machine with
/// a Store install: the alias by its data, the package path by the installed package's directory.
///
/// MUTATION: answer `None` from `installed_family` — the package path is in no package.
#[test]
fn the_store_alias_and_its_package_path_are_one_package() {
    let Some((alias, target)) = store_pwsh() else {
        skipped("the_store_alias_and_its_package_path_are_one_package");
        return;
    };
    let by_alias = package_family_of(&alias).expect("the alias names its package");
    assert_eq!(package_family_of(&target), Some(by_alias.clone()));
    assert!(
        by_alias.name().starts_with("microsoft.powershell_"),
        "{by_alias:?}"
    );
}

/// RED — **a folder that only looks like a package's is not one**: a directory named for the
/// Store package's full name, outside the place Windows installed it, holds no packaged program.
///
/// MUTATION: drop the `same_directory` comparison in `installed_family` — the look-alike is
/// answered as the package.
#[test]
fn a_folder_named_like_a_package_is_not_one() {
    let Some((_, target)) = store_pwsh() else {
        skipped("a_folder_named_like_a_package_is_not_one");
        return;
    };
    let full_name = target
        .parent()
        .and_then(Path::file_name)
        .expect("the package's own folder");
    let root = bt_testpath::temp_path("packaged-look-alike");
    let folder = root.join("副本").join(full_name);
    std::fs::create_dir_all(&folder).expect("make the look-alike folder");
    let program = folder.join("pwsh.exe");
    std::fs::write(&program, b"").expect("make the look-alike program");
    let answer = package_family_of(&program);
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(answer, None);
}

/// **Windows' own programs are in no package**: Windows PowerShell 5.1 and `cmd.exe`, the two a
/// pane runs when PowerShell 7 is not there, and a program that does not exist.
#[test]
fn the_inbox_shells_are_in_no_package() {
    let system = Path::new(&std::env::var_os("SystemRoot").expect("Windows sets SystemRoot"))
        .join("System32");
    for program in [
        system
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe"),
        system.join("cmd.exe"),
        system.join("不存在的程序.exe"),
    ] {
        assert_eq!(package_family_of(&program), None, "{}", program.display());
    }
}

/// RED (T-CHECK-NEVER-OVERLAPS-A-PACKAGED-BIRTH, the experiment's open item) — **a probe of the
/// Store's app execution alias is born on the packaged road**: Windows refuses its job list with
/// `ERROR_ACCESS_DENIED` (5), not the status form, and the probe is still created suspended, put
/// in its job and resumed — it runs and answers.
///
/// MUTATION: match only `PACKAGED_JOB_LIST_REFUSAL` in `packaged_job_list_refusal` — the start
/// through the alias fails with access denied after one attempt and the probe never runs (main
/// before this ticket: every PowerShell 7 check through the alias).
#[test]
fn a_probe_of_the_store_alias_is_born_on_the_packaged_road() {
    let Some((alias, _)) = store_pwsh() else {
        skipped("a_probe_of_the_store_alias_is_born_on_the_packaged_road");
        return;
    };
    let seam = crate::ProbeBirthSeam::default();
    let attempts = seam.attempts.clone();
    let mut command = crate::quiet_command(&alias);
    command.args(["-NoProfile", "-NonInteractive", "-Command", "exit 7"]);
    let child = crate::spawn_probe_born(
        &command,
        crate::ProbeStdio::ANSWER,
        None,
        crate::probe_job(),
        seam,
    )
    .expect("the alias's probe starts");
    let output = child.wait_with_output().expect("collect the alias's probe");
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(attempts.get(), 2, "refused once, then born suspended");
}

/// **Access denied is the packaged refusal only for a program in a package**: for Windows' own
/// `cmd.exe` it is that start's own answer, as is any other code; the status form is the packaged
/// refusal whatever the program.
///
/// MUTATION: answer `true` for every 5 — `cmd.exe`'s access denied is taken for the packaged road.
#[test]
fn only_a_packaged_program_s_access_denied_is_the_packaged_refusal() {
    let cmd = Path::new(&std::env::var_os("SystemRoot").expect("Windows sets SystemRoot"))
        .join("System32")
        .join("cmd.exe");
    let denied = std::io::Error::from_raw_os_error(5);
    assert!(!crate::packaged_job_list_refusal(&denied, &cmd));
    assert!(!crate::packaged_job_list_refusal(
        &std::io::Error::from_raw_os_error(2),
        &cmd
    ));
    assert!(crate::packaged_job_list_refusal(
        &std::io::Error::from_raw_os_error(crate::PACKAGED_JOB_LIST_REFUSAL),
        &cmd
    ));
    let Some((alias, target)) = store_pwsh() else {
        skipped("only_a_packaged_program_s_access_denied_is_the_packaged_refusal");
        return;
    };
    assert!(crate::packaged_job_list_refusal(&denied, &alias));
    assert!(crate::packaged_job_list_refusal(&denied, &target));
}

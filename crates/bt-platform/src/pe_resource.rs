//! **A resource read out of an executable that is never run** — the E-14 door
//! (`docs/plans/design/self-update-2026-09-16.md`, revision (b), F-4 and
//! experiment E-14; 0.4.6 ticket U-14).
//!
//! The release manifest travels inside the new `folio.exe` as an `RT_RCDATA`
//! resource (`bt_winres::release_manifest::RESOURCE_NAME`), signed by that
//! file's own signature. The updater reads it before anything is moved, from a
//! file it has just expanded and has no reason to trust yet, so the read must
//! not run a byte of it. `LoadLibraryExW` with `LOAD_LIBRARY_AS_DATAFILE |
//! LOAD_LIBRARY_AS_IMAGE_RESOURCE` maps the file as data: no import is
//! resolved, no entry point or TLS callback runs, and the module handle it
//! returns is good only for the resource functions. That is the call
//! `scripts/release/release-manifest.ps1`'s `Read-ReleaseManifestText` makes
//! (U-9), and this is its Rust twin: `FindResourceW` → `SizeofResource` →
//! `LoadResource` → `LockResource` → a copy → `FreeLibrary`.
//!
//! The loader's reads of the file are its own, so the call is charged to
//! [`crate::file_reads`]' `Lane::Update` as one opaque load.
//!
//! A build with no Windows arm refuses with [`io::ErrorKind::Unsupported`]: the
//! archive this reads the executable out of is Windows', and nothing else
//! reads a PE resource.

use std::io;
use std::path::Path;

/// **The bytes of the `RT_RCDATA` resource called `name` in the executable at
/// `path`**, read without running it; at most `limit` bytes.
///
/// # Errors
///
/// [`io::ErrorKind::NotFound`] when the file carries no such resource;
/// [`io::ErrorKind::InvalidData`] when it is longer than `limit`; the loader's
/// own error when the file cannot be mapped as an image (it is not a PE, or it
/// is truncated); [`io::ErrorKind::Unsupported`] off Windows.
pub fn read_rcdata(path: &Path, name: &str, limit: usize) -> io::Result<Vec<u8>> {
    crate::file_reads::opaque(crate::file_reads::Lane::Update, || {
        arm::read_rcdata(path, name, limit)
    })
}

#[cfg(windows)]
mod arm {
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{
        FindResourceW, LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE, LoadLibraryExW,
        LoadResource, LockResource, SizeofResource,
    };
    use windows::core::PCWSTR;

    /// `MAKEINTRESOURCE(10)`.
    const RT_RCDATA: PCWSTR = PCWSTR(10 as _);

    fn wide(text: &std::ffi::OsStr) -> io::Result<Vec<u16>> {
        let mut value: Vec<u16> = text.encode_wide().collect();
        if value.contains(&0) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "NUL in a name"));
        }
        value.push(0);
        Ok(value)
    }

    fn os_error(error: &windows::core::Error) -> io::Error {
        let code = error.code().0.cast_unsigned();
        if code & 0xFFFF_0000 == 0x8007_0000 {
            io::Error::from_raw_os_error((code & 0xFFFF).cast_signed())
        } else {
            io::Error::from_raw_os_error(code.cast_signed())
        }
    }

    pub(super) fn read_rcdata(path: &Path, name: &str, limit: usize) -> io::Result<Vec<u8>> {
        let file = wide(path.as_os_str())?;
        let resource = wide(std::ffi::OsStr::new(name))?;
        // SAFETY: the path is NUL-terminated and lives across the call. With
        // these two flags the loader maps the file as data and runs nothing.
        let module = unsafe {
            LoadLibraryExW(
                PCWSTR(file.as_ptr()),
                None,
                LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
            )
        }
        .map_err(|error| os_error(&error))?;

        let read = (|| {
            // SAFETY: `module` is the live data-file module loaded above and
            // the name is NUL-terminated; the type is an integer resource id.
            let found =
                unsafe { FindResourceW(Some(module), PCWSTR(resource.as_ptr()), RT_RCDATA) };
            if found.is_invalid() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("{} carries no RCDATA resource {name}", path.display()),
                ));
            }
            // SAFETY: `found` is a resource of `module`, both live.
            let size = unsafe { SizeofResource(Some(module), found) } as usize;
            if size > limit {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("the resource {name} is {size} bytes, more than {limit}"),
                ));
            }
            // SAFETY: as above; the loaded block lives as long as `module`.
            let loaded =
                unsafe { LoadResource(Some(module), found) }.map_err(|error| os_error(&error))?;
            // SAFETY: `loaded` was returned by `LoadResource` for this module.
            let start = unsafe { LockResource(loaded) }.cast::<u8>();
            if start.is_null() {
                return Err(io::Error::other(format!(
                    "the resource {name} could not be locked"
                )));
            }
            // SAFETY: `LockResource` points at `size` readable bytes that live
            // until `module` is freed, which is after this copy.
            Ok(unsafe { std::slice::from_raw_parts(start, size) }.to_vec())
        })();

        // SAFETY: `module` was loaded above and nothing refers to it after the
        // copy.
        unsafe {
            let _ = FreeLibrary(module);
        }
        read
    }
}

#[cfg(not(windows))]
mod arm {
    use std::io;
    use std::path::Path;

    pub(super) fn read_rcdata(path: &Path, name: &str, _limit: usize) -> io::Result<Vec<u8>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "reading the resource {name} out of {} needs Windows' loader",
                path.display()
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::read_rcdata;

    fn scratch(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "bt-platform-pe-resource-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// RED (U-14) — **a file that is not an executable, and an executable
    /// without the resource, are refused, and nothing is run to find out.**
    ///
    /// The door's positive arm is held by `bt-app`'s
    /// `update_archive::tests::the_manifest_is_read_out_of_a_built_folio_exe_without_running_it`,
    /// on the real `folio.exe` this workspace builds. Here the two refusals a
    /// hostile or damaged archive can produce are held on real files: text
    /// named `folio.exe` is not an image the loader maps, and this test's own
    /// executable is a PE with no `FOLIO_RELEASE_MANIFEST` in it — mapped as
    /// data, searched, and refused as not found, not run a second time. Off
    /// Windows the door says it has no loader.
    ///
    /// MUTATION: return `Ok(Vec::new())` when `FindResourceW` finds nothing and
    /// the second refusal is an empty manifest instead.
    #[test]
    fn pe_resource_a_file_without_the_resource_is_refused() {
        let root = scratch("refused");
        let text = root.join("folio.exe");
        std::fs::write(&text, b"folio-release-manifest 1\n").unwrap();
        let this = std::env::current_exe().unwrap();

        let not_a_pe = read_rcdata(&text, "FOLIO_RELEASE_MANIFEST", 65_536);
        let no_resource = read_rcdata(&this, "FOLIO_RELEASE_MANIFEST", 65_536);
        if cfg!(windows) {
            assert!(not_a_pe.is_err(), "text is not an image: {not_a_pe:?}");
            let error = no_resource.expect_err("this test carries no manifest");
            assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{error}");
        } else {
            for result in [not_a_pe, no_resource] {
                let error = result.expect_err("no loader here");
                assert_eq!(error.kind(), std::io::ErrorKind::Unsupported, "{error}");
            }
        }
        std::fs::remove_dir_all(&root).unwrap();
    }
}

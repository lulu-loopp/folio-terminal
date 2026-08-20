//! The smallest honest `IDataObject`.
//!
//! Gate 4 has to hand WebView2 a real OLE drag payload, and the ways to get one
//! without writing it are all worse: `OleGetClipboard` would trample the user's
//! clipboard, and a shell item array needs a file to exist. Eighty lines of COM
//! is cheaper than either, and it makes the payload's exact shape — CF_HDROP
//! plus CF_UNICODETEXT, the two formats a file dropped from Explorer carries —
//! part of the experiment rather than a guess.

use std::cell::RefCell;
use windows::Win32::Foundation::{DATA_S_SAMEFORMATETC, DV_E_FORMATETC, E_NOTIMPL, HGLOBAL, S_OK};
use windows::Win32::System::Com::{
    DATADIR, DVASPECT_CONTENT, FORMATETC, IAdviseSink, IDataObject, IDataObject_Impl,
    IEnumFORMATETC, IEnumFORMATETC_Impl, IEnumSTATDATA, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{CF_HDROP, CF_UNICODETEXT};
use windows::Win32::UI::Shell::DROPFILES;
use windows::core::{HRESULT, Ref, implement};

/// A payload that looks like one file dragged out of Explorer.
#[implement(IDataObject)]
pub struct FileDrop {
    path: Vec<u16>,
}

impl FileDrop {
    /// Build the payload and hand back the `IDataObject` interface directly —
    /// this never returns a bare `FileDrop`, so it is named for what it
    /// returns rather than `new`.
    pub fn create(path: &std::path::Path) -> IDataObject {
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect::<Vec<u16>>();
        // A double NUL: `DROPFILES` carries a NUL-separated list terminated by
        // an empty entry, and a single terminator is the classic way to hand a
        // drop target one file and a buffer overrun.
        wide.push(0);
        wide.push(0);
        FileDrop { path: wide }.into()
    }

    fn hdrop(&self) -> windows::core::Result<STGMEDIUM> {
        unsafe {
            let header = size_of::<DROPFILES>();
            let bytes = header + self.path.len() * 2;
            let handle = GlobalAlloc(GMEM_MOVEABLE, bytes)?;
            let base = GlobalLock(handle).cast::<u8>();
            std::ptr::write_bytes(base, 0, bytes);
            let drop_files = base.cast::<DROPFILES>();
            (*drop_files).pFiles = header as u32;
            (*drop_files).fWide = true.into();
            std::ptr::copy_nonoverlapping(
                self.path.as_ptr(),
                base.add(header).cast::<u16>(),
                self.path.len(),
            );
            let _ = GlobalUnlock(handle);
            Ok(medium(handle))
        }
    }

    fn text(&self) -> windows::core::Result<STGMEDIUM> {
        unsafe {
            // The same path as plain text, which is the second format Explorer
            // offers and the one a web page's `text/plain` maps to.
            let bytes = self.path.len() * 2;
            let handle = GlobalAlloc(GMEM_MOVEABLE, bytes)?;
            let base = GlobalLock(handle).cast::<u16>();
            std::ptr::copy_nonoverlapping(self.path.as_ptr(), base, self.path.len());
            let _ = GlobalUnlock(handle);
            Ok(medium(handle))
        }
    }
}

fn medium(handle: HGLOBAL) -> STGMEDIUM {
    STGMEDIUM {
        tymed: TYMED_HGLOBAL.0 as u32,
        u: STGMEDIUM_0 { hGlobal: handle },
        pUnkForRelease: std::mem::ManuallyDrop::new(None),
    }
}

use std::os::windows::ffi::OsStrExt as _;

impl IDataObject_Impl for FileDrop_Impl {
    fn GetData(&self, format: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
        let format = unsafe { &*format };
        match u32::from(format.cfFormat) {
            value if value == CF_HDROP.0 as u32 => self.hdrop(),
            value if value == CF_UNICODETEXT.0 as u32 => self.text(),
            _ => Err(DV_E_FORMATETC.into()),
        }
    }

    fn GetDataHere(
        &self,
        _format: *const FORMATETC,
        _medium: *mut STGMEDIUM,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn QueryGetData(&self, format: *const FORMATETC) -> HRESULT {
        let format = unsafe { &*format };
        let known = u32::from(format.cfFormat) == CF_HDROP.0 as u32
            || u32::from(format.cfFormat) == CF_UNICODETEXT.0 as u32;
        if known && (format.tymed & TYMED_HGLOBAL.0 as u32) != 0 {
            S_OK
        } else {
            DV_E_FORMATETC
        }
    }

    fn GetCanonicalFormatEtc(&self, _format: *const FORMATETC, out: *mut FORMATETC) -> HRESULT {
        unsafe { (*out).ptd = std::ptr::null_mut() };
        DATA_S_SAMEFORMATETC
    }

    fn SetData(
        &self,
        _format: *const FORMATETC,
        _medium: *const STGMEDIUM,
        _release: windows::core::BOOL,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn EnumFormatEtc(&self, direction: u32) -> windows::core::Result<IEnumFORMATETC> {
        if direction != DATADIR(1).0 as u32 {
            return Err(E_NOTIMPL.into());
        }
        Ok(Formats {
            index: RefCell::new(0),
        }
        .into())
    }

    fn DAdvise(
        &self,
        _format: *const FORMATETC,
        _flags: u32,
        _sink: Ref<IAdviseSink>,
    ) -> windows::core::Result<u32> {
        Err(E_NOTIMPL.into())
    }

    fn DUnadvise(&self, _connection: u32) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
        Err(E_NOTIMPL.into())
    }
}

#[implement(IEnumFORMATETC)]
struct Formats {
    index: RefCell<usize>,
}

const OFFERED: [u16; 2] = [CF_HDROP.0, CF_UNICODETEXT.0];

impl IEnumFORMATETC_Impl for Formats_Impl {
    fn Next(&self, count: u32, out: *mut FORMATETC, fetched: *mut u32) -> HRESULT {
        let mut index = self.index.borrow_mut();
        let mut written = 0;
        while written < count as usize && *index < OFFERED.len() {
            unsafe {
                *out.add(written) = FORMATETC {
                    cfFormat: OFFERED[*index],
                    ptd: std::ptr::null_mut(),
                    dwAspect: DVASPECT_CONTENT.0,
                    lindex: -1,
                    tymed: TYMED_HGLOBAL.0 as u32,
                };
            }
            written += 1;
            *index += 1;
        }
        if !fetched.is_null() {
            unsafe { *fetched = written as u32 };
        }
        if written == count as usize {
            S_OK
        } else {
            windows::Win32::Foundation::S_FALSE
        }
    }

    fn Skip(&self, count: u32) -> windows::core::Result<()> {
        *self.index.borrow_mut() += count as usize;
        Ok(())
    }

    fn Reset(&self) -> windows::core::Result<()> {
        *self.index.borrow_mut() = 0;
        Ok(())
    }

    fn Clone(&self) -> windows::core::Result<IEnumFORMATETC> {
        Ok(Formats {
            index: RefCell::new(*self.index.borrow()),
        }
        .into())
    }
}

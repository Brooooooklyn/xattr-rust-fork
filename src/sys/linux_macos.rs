use std::ffi::{OsStr, OsString};
use std::io;
use std::mem;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::BorrowedFd;
use std::path::Path;

use rustix::fs as rfs;
use rustix::path::Arg;

use crate::util::allocate_loop;

use std::os::raw::c_char;

// Convert an `&mut [u8]` to an `&mut [c_char]`
#[inline]
fn as_listxattr_buffer(buf: &mut [u8]) -> &mut [c_char] {
    // SAFETY: u8 and i8 have the same size and alignment
    unsafe {
        &mut *(buf as *mut [u8] as *mut [c_char])
    }
}

/// An iterator over a set of extended attributes names.
pub struct XAttrs {
    data: Box<[u8]>,
    offset: usize,
}

impl Clone for XAttrs {
    fn clone(&self) -> Self {
        XAttrs {
            data: Vec::from(&*self.data).into_boxed_slice(),
            offset: self.offset,
        }
    }
    fn clone_from(&mut self, other: &XAttrs) {
        self.offset = other.offset;

        let mut data = mem::replace(&mut self.data, Box::new([])).into_vec();
        data.extend(other.data.iter().cloned());
        self.data = data.into_boxed_slice();
    }
}

// Yes, I could avoid these allocations on linux/macos. However, if we ever want to be freebsd
// compatible, we need to be able to prepend the namespace to the extended attribute names.
// Furthermore, borrowing makes the API messy.
impl Iterator for XAttrs {
    type Item = OsString;
    fn next(&mut self) -> Option<OsString> {
        let data = &self.data[self.offset..];
        if data.is_empty() {
            None
        } else {
            // always null terminated (unless empty).
            let end = data.iter().position(|&b| b == 0u8).unwrap();
            self.offset += end + 1;
            Some(OsStr::from_bytes(&data[..end]).to_owned())
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.data.len() == self.offset {
            (0, Some(0))
        } else {
            (1, None)
        }
    }
}

pub fn get_fd(fd: BorrowedFd<'_>, name: &OsStr) -> io::Result<Vec<u8>> {
    allocate_loop(|buf| {
        rfs::fgetxattr(fd, name, buf)
    })
}

pub fn set_fd(fd: BorrowedFd<'_>, name: &OsStr, value: &[u8]) -> io::Result<()> {
    rfs::fsetxattr(fd, name, value, rfs::XattrFlags::empty())?;
    Ok(())
}

pub fn remove_fd(fd: BorrowedFd<'_>, name: &OsStr) -> io::Result<()> {
    rfs::fremovexattr(fd, name)?;
    Ok(())
}

pub fn list_fd(fd: BorrowedFd<'_>) -> io::Result<XAttrs> {
    let vec = allocate_loop(|buf| {
        rfs::flistxattr(fd, as_listxattr_buffer(buf))
    })?;
    Ok(XAttrs {
        data: vec.into_boxed_slice(),
        offset: 0,
    })
}

pub fn get_path(path: &Path, name: &OsStr) -> io::Result<Vec<u8>> {
    let path = path.into_c_str()?;
    let name = name.into_c_str()?;

    allocate_loop(|buf| {
        #[cfg(target_os = "macos")]
        {
            // If an empty slice is passed to lgetxattr on macOS, it returns an error.
            // Might be a macOS bug, so work around it here by calling the libc manually.
            if buf.is_empty() {
                let ret = unsafe {
                    libc::getxattr(
                        (&*path).as_ptr(),
                        (&*name).as_ptr(),
                        std::ptr::null_mut(),
                        0,
                        0,
                        libc::XATTR_NOFOLLOW
                    )
                };

                if ret < 0 {
                    return Err(io::Error::last_os_error());
                } else {
                    return Ok(ret as usize);
                }
            }
        }

        let size = rfs::lgetxattr(&*path, &*name, buf)?;
        io::Result::Ok(size)
    })
}

pub fn set_path(path: &Path, name: &OsStr, value: &[u8]) -> io::Result<()> {
    rfs::lsetxattr(path, name, value, rfs::XattrFlags::empty())?;
    Ok(())
}

pub fn remove_path(path: &Path, name: &OsStr) -> io::Result<()> {
    rfs::lremovexattr(path, name)?;
    Ok(())
}

pub fn list_path(path: &Path) -> io::Result<XAttrs> {
    let path = path.as_cow_c_str()?;
    let vec = allocate_loop(|buf| {
        rfs::llistxattr(&*path, as_listxattr_buffer(buf))
    })?;
    Ok(XAttrs {
        data: vec.into_boxed_slice(),
        offset: 0,
    })
}

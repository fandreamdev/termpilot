//! Small, secret-safe boundary around Windows Credential Manager.
//!
//! The database keeps only a target-name reference. Password bytes are stored
//! as a generic, session-scoped Windows credential and are read only while an
//! SSH connection is being created.

#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("credential manager is unavailable")]
    Unavailable,
    #[error("credential was not found")]
    NotFound,
    #[error("credential contains invalid data")]
    Invalid,
}

#[cfg(windows)]
mod platform {
    use super::CredentialError;
    use std::{ptr, slice};
    use windows_sys::Win32::{
        Foundation::FILETIME,
        Security::Credentials::{
            CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_SESSION,
            CRED_TYPE_GENERIC,
        },
    };

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn write_password(target: &str, password: &str) -> Result<(), CredentialError> {
        if target.is_empty() || password.is_empty() || password.len() > 16 * 1024 {
            return Err(CredentialError::Invalid);
        }
        let mut target_name = wide(target);
        let mut username = wide("TermPilot");
        let mut blob = password.as_bytes().to_vec();
        let credential = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target_name.as_mut_ptr(),
            Comment: ptr::null_mut(),
            LastWritten: FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            },
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_SESSION,
            AttributeCount: 0,
            Attributes: ptr::null_mut(),
            TargetAlias: ptr::null_mut(),
            UserName: username.as_mut_ptr(),
        };
        // SAFETY: all mutable pointers above stay valid for the synchronous API call.
        if unsafe { CredWriteW(&credential, 0) } == 0 {
            return Err(CredentialError::Unavailable);
        }
        blob.fill(0);
        Ok(())
    }

    pub fn read_password(target: &str) -> Result<String, CredentialError> {
        let target_name = wide(target);
        let mut raw: *mut CREDENTIALW = ptr::null_mut();
        // SAFETY: target_name is NUL-terminated and raw is a valid out pointer.
        if unsafe { CredReadW(target_name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut raw) } == 0 {
            return Err(CredentialError::NotFound);
        }
        // SAFETY: CredReadW returned an allocation released by CredFree below.
        let result = unsafe {
            let credential = &*raw;
            if credential.CredentialBlob.is_null() || credential.CredentialBlobSize == 0 {
                Err(CredentialError::Invalid)
            } else {
                let bytes = slice::from_raw_parts(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                );
                std::str::from_utf8(bytes)
                    .map(str::to_owned)
                    .map_err(|_| CredentialError::Invalid)
            }
        };
        // SAFETY: raw came from CredReadW and is freed exactly once.
        unsafe { CredFree(raw.cast()) };
        result
    }

    pub fn delete_password(target: &str) -> Result<(), CredentialError> {
        let target_name = wide(target);
        // A missing old value is already the desired end state.
        // SAFETY: target_name remains valid for the duration of this synchronous call.
        let _ = unsafe { CredDeleteW(target_name.as_ptr(), CRED_TYPE_GENERIC, 0) };
        Ok(())
    }
}

#[cfg(not(windows))]
mod platform {
    use super::CredentialError;

    pub fn write_password(_target: &str, _password: &str) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable)
    }
    pub fn read_password(_target: &str) -> Result<String, CredentialError> {
        Err(CredentialError::NotFound)
    }
    pub fn delete_password(_target: &str) -> Result<(), CredentialError> {
        Ok(())
    }
}

pub use platform::{delete_password, read_password, write_password};

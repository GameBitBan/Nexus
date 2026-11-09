//! Windows: secrets are encrypted with DPAPI (readable only by the same Windows account), in the same
//! `dpapi:<base64>` format as the Python version. Other systems keep them in the user-only config file.

#[cfg(windows)]
mod imp {
    use base64::Engine;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };

    pub fn protect(text: &str) -> String {
        let mut data = text.as_bytes().to_vec();
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_mut_ptr() };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        let desc: Vec<u16> = "nexus\0".encode_utf16().collect();
        let ok = unsafe {
            CryptProtectData(&input, desc.as_ptr(), std::ptr::null(), std::ptr::null(), std::ptr::null(),
                             CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        };
        if ok == 0 {
            return text.to_string();
        }
        let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
        unsafe { LocalFree(out.pbData as _) };
        format!("dpapi:{}", base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub fn reveal(text: &str) -> String {
        let Some(b64) = text.strip_prefix("dpapi:") else { return text.to_string() };
        let Ok(mut raw) = base64::engine::general_purpose::STANDARD.decode(b64) else { return String::new() };
        let input = CRYPT_INTEGER_BLOB { cbData: raw.len() as u32, pbData: raw.as_mut_ptr() };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        let ok = unsafe {
            CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(),
                               CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        };
        if ok == 0 {
            return String::new();
        }
        let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
        unsafe { LocalFree(out.pbData as _) };
        String::from_utf8(bytes).unwrap_or_default()
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn protect(text: &str) -> String {
        text.to_string()
    }

    pub fn reveal(text: &str) -> String {
        // a config copied over from Windows can't be decrypted here
        if text.starts_with("dpapi:") { String::new() } else { text.to_string() }
    }
}

pub use imp::{protect, reveal};

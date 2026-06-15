#[cfg(target_os = "macos")]
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

use crate::error::Error;

#[derive(Clone, Debug)]
pub struct AppleKeychainManager(String);

impl AppleKeychainManager {
    pub fn new(service_name: &str) -> Self {
        AppleKeychainManager(service_name.to_string())
    }

    #[cfg(target_os = "macos")]
    pub fn store_password(&self, account: &str, password: &str) -> Result<(), Error> {
        set_generic_password(&self.0, account, password.as_bytes())?;

        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    pub fn store_password(&self, _account: &str, _password: &str) -> Result<(), Error> {
        Err(Error::Keychain(
            "Apple Keychain is only supported on macOS".to_string(),
        ))
    }

    #[cfg(target_os = "macos")]
    pub fn retrieve_password(&self, account: &str) -> Result<String, Error> {
        let password_bytes = get_generic_password(&self.0, account)?;

        let password = String::from_utf8(password_bytes)?;

        Ok(password)
    }

    #[cfg(not(target_os = "macos"))]
    pub fn retrieve_password(&self, _account: &str) -> Result<String, Error> {
        Err(Error::Keychain(
            "Apple Keychain is only supported on macOS".to_string(),
        ))
    }

    #[cfg(target_os = "macos")]
    pub fn delete_password(&self, account: &str) -> Result<(), Error> {
        delete_generic_password(&self.0, account)?;

        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    pub fn delete_password(&self, _account: &str) -> Result<(), Error> {
        Err(Error::Keychain(
            "Apple Keychain is only supported on macOS".to_string(),
        ))
    }
}

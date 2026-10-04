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

    fn service_name(&self) -> &str {
        &self.0
    }

    fn canonical_account(account: &str) -> String {
        account.to_ascii_lowercase()
    }

    #[cfg(target_os = "macos")]
    pub fn store_password(&self, account: &str, password: &str) -> Result<(), Error> {
        let account = Self::canonical_account(account);
        set_generic_password(self.service_name(), &account, password.as_bytes())?;

        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    pub fn store_password(&self, _account: &str, _password: &str) -> Result<(), Error> {
        let _account = Self::canonical_account(_account);
        Err(Error::Keychain(format!(
            "Apple Keychain service '{}' is only supported on macOS",
            self.service_name()
        )))
    }

    #[cfg(target_os = "macos")]
    pub fn retrieve_password(&self, account: &str) -> Result<String, Error> {
        let account = Self::canonical_account(account);
        tracing::debug!("Requesting stored password from Apple Keychain");
        let password_bytes = get_generic_password(self.service_name(), &account)?;
        tracing::debug!("Stored password retrieved from Apple Keychain");

        let password = String::from_utf8(password_bytes)?;

        Ok(password)
    }

    #[cfg(not(target_os = "macos"))]
    pub fn retrieve_password(&self, _account: &str) -> Result<String, Error> {
        let _account = Self::canonical_account(_account);
        Err(Error::Keychain(format!(
            "Apple Keychain service '{}' is only supported on macOS",
            self.service_name()
        )))
    }

    #[cfg(target_os = "macos")]
    pub fn delete_password(&self, account: &str) -> Result<(), Error> {
        let account = Self::canonical_account(account);
        delete_generic_password(self.service_name(), &account)?;

        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    pub fn delete_password(&self, _account: &str) -> Result<(), Error> {
        Err(Error::Keychain(format!(
            "Apple Keychain service '{}' is only supported on macOS",
            self.service_name()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::AppleKeychainManager;

    #[test]
    fn canonical_account_uses_lowercase_ascii_email() {
        assert_eq!(
            AppleKeychainManager::canonical_account("Seun.Aminu@EXAMPLE.COM"),
            "seun.aminu@example.com"
        );
    }
}

use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

use crate::error::Error;

#[derive(Clone)]
pub struct AppleKeychainManager(String);

impl AppleKeychainManager {
    pub fn new(service_name: &str) -> Self {
        AppleKeychainManager(service_name.to_string())
    }

    fn service_name(&self) -> &str {
        &self.0
    }

    pub fn store_password(&self, account: &str, password: &str) -> Result<(), Error> {
        set_generic_password(self.service_name(), account, password.as_bytes())?;

        Ok(())
    }

    pub fn retrieve_password(&self, account: &str) -> Result<String, Error> {
        let password_bytes = get_generic_password(self.service_name(), account)?;

        let password = String::from_utf8(password_bytes)?;

        Ok(password)
    }

    pub fn delete_password(&self, account: &str) -> Result<(), Error> {
        delete_generic_password(self.service_name(), account)?;

        Ok(())
    }
}

//! Telegram account state. The runtime is compiled only with the `telegram`
//! feature; the login state is always present so the window can show it.

#[cfg(feature = "telegram")]
pub mod convert;
#[cfg(feature = "telegram")]
pub mod session;
#[cfg(feature = "telegram")]
pub mod worker;

/// Where the Telegram login stands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Auth {
    #[default]
    Starting,
    NeedCredentials {
        error: Option<String>,
    },
    NeedPhone {
        error: Option<String>,
    },
    NeedCode {
        error: Option<String>,
    },
    NeedPassword {
        hint: Option<String>,
        error: Option<String>,
    },
    Authorized,
    Failed(String),
}

impl Auth {
    pub fn error(&self) -> Option<&str> {
        match self {
            Self::NeedCredentials { error }
            | Self::NeedPhone { error }
            | Self::NeedCode { error }
            | Self::NeedPassword { error, .. } => error.as_deref(),
            Self::Failed(error) => Some(error),
            Self::Starting | Self::Authorized => None,
        }
    }
}

/// The values typed into the Telegram login form. Never logged: it holds the
/// API hash, the code, and the password.
#[derive(Clone, Default)]
pub struct Login {
    pub api_id: String,
    pub api_hash: String,
    pub phone: String,
    pub code: String,
    pub password: String,
    pub auth: Auth,
}

impl Login {
    pub fn needs_credentials(&self) -> bool {
        matches!(self.auth, Auth::NeedCredentials { .. })
    }

    pub fn needs_code(&self) -> bool {
        matches!(self.auth, Auth::NeedCode { .. })
    }

    pub fn needs_password(&self) -> bool {
        matches!(self.auth, Auth::NeedPassword { .. })
    }
}

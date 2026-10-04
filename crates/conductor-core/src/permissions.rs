use crate::{domain::*, Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    FileRead,
    FileWrite,
    FileDelete,
    Terminal,
    GitRead,
    GitWrite,
    Network,
    Mcp,
    SecretsUse,
    Browser,
    Computer,
    Install,
    Remote,
}
impl Capability {
    pub fn name(self) -> &'static str {
        match self {
            Self::FileRead => "filesystem.read",
            Self::FileWrite => "filesystem.write",
            Self::FileDelete => "filesystem.delete",
            Self::Terminal => "terminal.execute",
            Self::GitRead => "git.read",
            Self::GitWrite => "git.write",
            Self::Network => "network",
            Self::Mcp => "mcp",
            Self::SecretsUse => "secrets.use",
            Self::Browser => "browser",
            Self::Computer => "computer.control",
            Self::Install => "install.software",
            Self::Remote => "remote.host",
        }
    }
    pub fn read_only(self) -> bool {
        matches!(self, Self::FileRead | Self::GitRead)
    }
}
pub fn authorize(
    mode: Mode,
    settings: &Settings,
    capability: Capability,
    approved: bool,
) -> Result<()> {
    if mode == Mode::Plan && !capability.read_only() && capability != Capability::Network {
        return Err(Error::Denied(
            "Plan mode cannot change files or run commands. Switch to Agent or Goal.".into(),
        ));
    }
    if capability.read_only() || approved {
        return Ok(());
    }
    match settings.permission {
        PermissionLevel::FullAccess => Ok(()),
        PermissionLevel::AutoApprove
            if settings.auto_approve.iter().any(|s| s == capability.name()) =>
        {
            Ok(())
        }
        _ => Err(Error::Approval(capability.name().into())),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plan_cannot_mutate_even_with_full_access_or_approval() {
        let settings = Settings {
            permission: PermissionLevel::FullAccess,
            ..Default::default()
        };
        for cap in [
            Capability::FileWrite,
            Capability::FileDelete,
            Capability::Terminal,
            Capability::GitWrite,
            Capability::Computer,
            Capability::Mcp,
            Capability::SecretsUse,
        ] {
            assert!(matches!(
                authorize(Mode::Plan, &settings, cap, true),
                Err(Error::Denied(_))
            ));
        }
        assert!(authorize(Mode::Plan, &settings, Capability::FileRead, false).is_ok());
    }
    #[test]
    fn auto_approval_is_scoped_and_revocation_is_immediate() {
        let mut settings = Settings {
            permission: PermissionLevel::AutoApprove,
            auto_approve: vec!["filesystem.write".into()],
            ..Default::default()
        };
        assert!(authorize(Mode::Agent, &settings, Capability::FileWrite, false).is_ok());
        assert!(authorize(Mode::Agent, &settings, Capability::Terminal, false).is_err());
        settings.permission = PermissionLevel::Ask;
        assert!(authorize(Mode::Agent, &settings, Capability::FileWrite, false).is_err());
    }
}

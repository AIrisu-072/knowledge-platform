//! Organization-only configuration. Document's existing public profiles are unchanged.
use crate::OrganizationProfile;
use document_server::config::{Command, ConfigSource, RuntimeConfig};

pub struct OrganizationConfig {
    profile: OrganizationProfile,
    document: RuntimeConfig,
}
struct DocumentEnvironment<'a, S> {
    source: &'a S,
    profile: OrganizationProfile,
}
impl<S: ConfigSource> ConfigSource for DocumentEnvironment<'_, S> {
    fn get(&self, name: &str) -> Option<String> {
        match name {
            // These select the existing Document runtime's Human GUI/storage shape;
            // actual identity is supplied separately by the trusted composition root.
            "KP_RUNTIME_MODE" => Some("poc".into()),
            "KP_IDENTITY_PROFILE" => Some("poc-human".into()),
            "KP_POC_ALLOW_NON_LOOPBACK" => Some("false".into()),
            "KP_BIND" => Some(
                self.source
                    .get(name)
                    .unwrap_or_else(|| self.profile.default_bind().into()),
            ),
            _ => self.source.get(name),
        }
    }
}
impl OrganizationConfig {
    pub fn from_env(source: &impl ConfigSource, command: Command) -> Result<Self, &'static str> {
        if source.get("KP_RUNTIME_MODE").as_deref() != Some("organization-synthetic") {
            return Err("KP_RUNTIME_MODE must explicitly select organization-synthetic");
        }
        let profile = OrganizationProfile::parse(
            source
                .get("KP_ORGANIZATION_PROFILE")
                .as_deref()
                .unwrap_or(""),
        )?;
        let document = RuntimeConfig::from_env(&DocumentEnvironment { source, profile }, command)
            .map_err(|_| "Organization PoC configuration is invalid")?;
        Ok(Self { profile, document })
    }
    pub const fn profile(&self) -> OrganizationProfile {
        self.profile
    }
    pub const fn document(&self) -> &RuntimeConfig {
        &self.document
    }
}

pub const KEYCLOAK_URL: &str = "https://auth.dverse.yordanmitev.me";
pub const KEYCLOAK_REALM: &str = "master";
pub const CLIENT_ID: &str = "step-ca";
pub const CLIENT_SECRET: &str = "oeWYn8BLhMsAt7j9qG7qEwIWATnBepAr";
pub const CA_URL: &str = "https://ca.dverse.yordanmitev.me:9000";
pub const ROUTER_LISTEN: &str = "tls/0.0.0.0:7447";
pub const ROUTER_PORT: u16 = 7447;
/// Keycloak client used for user self-registration.
/// The service account for this client must have the manage-users role
/// scoped to create-only; it cannot read, modify, or delete existing users.
pub const REGISTRATION_CLIENT_ID: &str = "dverse-registration";
pub const REGISTRATION_CLIENT_SECRET: &str = env!("DVERSE_REG_SECRET");

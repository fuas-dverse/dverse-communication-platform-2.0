pub const KEYCLOAK_URL: &str = "https://auth.dverse.yordanmitev.me";
pub const KEYCLOAK_REALM: &str = "dverse";
pub const CLIENT_ID: &str = "step-ca";
pub const CLIENT_SECRET: &str = "oeWYn8BLhMsAt7j9qG7qEwIWATnBepAr";
pub const CA_URL: &str = "https://ca.dverse.yordanmitev.me:9000";
pub const ROUTER_LISTEN: &str = "tls/0.0.0.0:7447";
pub const ROUTER_PORT: u16 = 7447;
/// Service-account client for user self-registration (manage-users scope, dverse realm).
/// Secret is managed in sops and injected into Keycloak at deploy time.
pub const REGISTRATION_CLIENT_ID: &str = "dverse-registration";
pub const REGISTRATION_CLIENT_SECRET: &str = "Xv2kR8nQ5mW4jT7eBpL3hC9gF6dA0sYz";

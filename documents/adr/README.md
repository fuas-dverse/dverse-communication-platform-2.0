# Architecture Decision Records

| ADR                     | Title                                                                              | Status |
|-------------------------|------------------------------------------------------------------------------------|--------|
| [ADR-001](./ADR-001.md) | Platform Architecture and Technology Stack                                         | Accepted |
| [ADR-002](./ADR-002.md) | Zenoh as the Agent Messaging Layer                                                 | Accepted |
| [ADR-003](./ADR-003.md) | PKI and mTLS Transport Security                                                    | Accepted |
| [ADR-004](./ADR-004.md) | Observable Multi-Agent System with Zenoh Communication                             | Accepted |
| [ADR-005](./ADR-005.md) | Configuring Avahi for the Local Zenoh Router Domain                                | Superseded by ADR-011 |
| [ADR-006](./ADR-006.md) | `src/` as the Unified Home for All Runtime Components                              | Accepted |
| [ADR-007](./ADR-007.md) | `bot_framework` as the Shared Library for All Nodes                                | Accepted |
| [ADR-008](./ADR-008.md) | Zenoh Client Mode for Agent Nodes                                                  | Accepted |
| [ADR-009](./ADR-009.md) | Intermediate CA Certificate as the TLS Trust Anchor for Nodes                      | Accepted |
| [ADR-010](./ADR-010.md) | Keycloak `preferred_username` as Cert CN and ACL Identity                          | Accepted |
| [ADR-011](./ADR-011.md) | Pure Rust mDNS-SD via `mdns-sd`                                                    | Superseded by ADR-012 |
| [ADR-012](./ADR-012.md) | Platform-conditional DNS-SD — avahi D-Bus on Linux, `mdns-sd` elsewhere            | Superseded by ADR-013 |
| [ADR-013](./ADR-013.md) | `mdns-sd` on every platform — drop the avahi D-Bus backend                         | Accepted |
| [ADR-014](./ADR-014.md) | Split `discovery.rs` into `common` / `announcing` / `browsing` submodules          | Accepted |
| [ADR-015](./ADR-015.md) | `SessionRole` and DNS-SD session filter                                            | Accepted |
| [ADR-016](./ADR-016.md) | Force certificate renewal when the cached cert's CN doesn't match the operator     | Accepted |
| [ADR-017](./ADR-017.md) | Present the router's client certificate on outgoing inter-router TLS connections   | Accepted |
| [ADR-018](./ADR-018.md) | Per-node agent inventory with heartbeat and stale eviction                         | Accepted |
| [ADR-019](./ADR-019.md) | Structured tracing for all log output across the workspace                         | Accepted |
| [ADR-020](./ADR-020.md) | Split agent topics into `Publishes` and `Subscribes`                               | Accepted |
| [ADR-021](./ADR-021.md) | Trim binary size by pruning unused dependency features                             | Accepted |
| [ADR-022](./ADR-022.md) | Consolidate the router's mTLS identity into a single `tls_identity` helper         | Accepted |
| [ADR-027](./ADR-027.md) | Agent-to-Agent Communication over Zenoh Pub/Sub                                    | Accepted |

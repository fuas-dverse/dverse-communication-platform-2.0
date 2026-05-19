# Architecture Decision Records

ADRs 001–005 are in LaTeX format (`.tex`). ADRs 006 onwards are in Markdown (`.md`).

| ADR | Title | Status |
|-----|-------|--------|
| [ADR-001](ADR-001.tex) | Platform Architecture and Technology Stack | Accepted |
| [ADR-002](ADR-002.tex) | Zenoh as the Agent Messaging Layer | Accepted |
| [ADR-003](ADR-003.tex) | PKI and mTLS Transport Security | Accepted |
| [ADR-004](ADR-004.tex) | Observable Multi-Agent System with Zenoh Communication | Accepted |
| [ADR-005](ADR-005.tex) | Configuring Avahi for the Local Zenoh Router Domain | Superseded by ADR-011 |
| [ADR-006](ADR-006.md) | `src/` as the Unified Home for All Runtime Components | Accepted |
| [ADR-007](ADR-007.md) | `bot_framework` as the Shared Library for All Nodes | Accepted |
| [ADR-008](ADR-008.md) | Zenoh Client Mode for Agent Nodes | Accepted |
| [ADR-009](ADR-009.md) | Intermediate CA Certificate as the TLS Trust Anchor for Nodes | Accepted |
| [ADR-010](ADR-010.md) | Keycloak `preferred_username` as Cert CN and ACL Identity | Accepted |
| [ADR-011](ADR-011.md) | Pure Rust mDNS-SD via `mdns-sd` (supersedes ADR-005) | Accepted |
<!--
Maintainer note: this policy relies on GitHub Private Vulnerability Reporting.
Enable it in the repository's Settings → Code security → "Private vulnerability
reporting", otherwise the "Report a vulnerability" button below does not appear.
-->

# Security Policy

MellowMesh coordinates AI agents and the humans who approve their actions, so we
take reports about its security seriously. Thank you for helping keep its users
safe.

## Supported versions

MellowMesh is pre-1.0. Security fixes are made on the latest release line only.

| Version | Supported |
| :--- | :--- |
| 0.1.x | Yes |
| < 0.1 | No |

## Reporting a vulnerability

**Please do not open a public GitHub issue, discussion, or pull request for a
security vulnerability.** Public reports put every user at risk before a fix is
available.

Report privately through GitHub's Private Vulnerability Reporting:

1. Open the repository's **Security** tab.
2. Click **Report a vulnerability**.

Direct link: <https://github.com/yannickhuchard/mellowmesh/security/advisories/new>

The report is visible only to you and the maintainers. We will discuss the issue,
collaborate on a fix, and (if needed) request a CVE through the same private
advisory.

### What to include

The more of this you can provide, the faster we can act:

- The affected component (see [Scope](#scope)) and version or commit.
- The configuration involved: open mode or `--require-auth`, relay in use,
  connectors enabled, operating system, and how MellowMesh was installed.
- A description of the issue and its impact: what an attacker can do, and what
  access they need first (local user, agent token, network position, relay
  operator, a web page the victim visits, ...).
- Step-by-step reproduction instructions or a minimal proof of concept.
- Any suggested fix or mitigation.
- Whether, and how, you would like to be credited.

## Our response

| Stage | Target |
| :--- | :--- |
| Acknowledge your report | within 72 hours |
| Initial assessment (validity, severity, affected versions) | within 7 days |
| Fix released | 30 days for critical/high severity, up to 90 days for medium/low |

We will keep you informed of progress, and tell you if we need more time or
disagree with the severity assessment.

## Coordinated disclosure

- We ask that you keep the details private until a fix is released, or until
  90 days after your report, whichever comes first. 90 days is the maximum
  embargo; we will not ask you to wait longer.
- Once a fixed release is available we publish a GitHub Security Advisory
  describing the issue, affected versions, and how to upgrade.
- We credit reporters in the advisory and release notes unless you prefer to
  remain anonymous.

## Scope

In scope:

- **Daemon** (`mellowmeshd`): HTTP/WebSocket API, authentication and token
  scopes, decision integrity, the web dashboard, storage, peering.
- **Relay** (`mellowmesh-relay`): hub registration, request forwarding,
  end-to-end encryption.
- **SDKs**: the Rust client and the WASM/browser SDK.
- **CLI and MCP server** (`mellowmesh`, `mellowmesh mcp`), including the tools
  exposed to AI agents.
- **Connectors**: Telegram, Discord, and Teams bridges.
- **Packaging**: the MSI/DEB/DMG installers, the systemd unit, packaging scripts,
  and release workflows.

Out of scope:

- **Open mode on a shared host.** Without `--require-auth`, the daemon trusts
  every client on localhost by design; this mode is documented as suitable only
  for a single-user, trusted machine (see [docs/security.md](docs/security.md)).
  Reports that another local user can act on an open-mode daemon are expected
  behaviour. (Bypassing `--require-auth`, or reaching an open-mode daemon from a
  web page or another machine, *is* in scope.)
- Behaviour of AI models themselves (e.g. an agent choosing to follow a prompt
  injection), unless MellowMesh grants that agent more authority than its token
  allows.
- Vulnerabilities in third-party dependencies that are already publicly known
  and tracked upstream (please still tell us if MellowMesh is exploitable
  through them).
- Denial of service that requires a valid owner token, social engineering, and
  physical access to a user's machine.

## Safe harbor

We consider security research carried out in good faith under this policy to be
authorized. We will not pursue or support legal action against you for:

- testing MellowMesh on systems you own or are explicitly allowed to test;
- accessing data only to the extent needed to demonstrate the issue;
- reporting the issue to us privately and giving us reasonable time to fix it
  before any public disclosure, as described above.

Please do not access, modify, or delete other people's data, degrade services
you do not own (including public relays), or use social engineering. If in
doubt about whether something is allowed, ask in your private report first.

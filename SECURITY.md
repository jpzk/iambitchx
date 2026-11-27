# Security Policy

## Supported versions

All released versions of iambitchx receive security fixes: 0.1.x and every
later version. Fixes land on `main` and ship in a new release; upgrade to the
latest release to get them.

## Reporting a vulnerability

Please do **not** open a public GitHub issue, pull request or discussion for
a security vulnerability.

Report it privately, using coordinated disclosure:

- Email **jendrik@madewithtea.com** (preferred), or
- use GitHub private vulnerability reporting: the repository's **Security**
  tab -> **Report a vulnerability**.

Please include:

- affected version or commit hash
- platform (OS, architecture, terminal) and how you installed iambitchx
- homeserver type and version (Synapse, Conduit/conduwuit/continuwuity,
  Dendrite, matrix.org, ...)
- step-by-step reproduction
- impact: what an attacker can do, and from which position (malicious
  homeserver, malicious room member, local user, network attacker, ...)
- a proof of concept, if you have one

## Response timeline

- Acknowledgement within 72 hours.
- Triage and initial assessment within 7 days.
- Fix and published advisory targeted within 90 days of the report.
- Disclosure is coordinated with you: we agree on a date before anything is
  made public.
- You are credited in the advisory and release notes unless you prefer not to
  be.

## Scope

In scope:

- iambitchx client code in this repository
- end-to-end encryption handling (key storage, verification, trust decisions,
  how encrypted and unverified content is displayed)
- login, OAuth/OIDC, SSO and device-link flows
- session, token and credential storage on disk
- release artifacts and their build provenance (checksums, attestations, SBOM)

## Out of scope

- Vulnerabilities in upstream projects: matrix-rust-sdk, iamb, or
  homeserver implementations. Please report those upstream, but also tell
  us so we can ship a fix or a workaround.
- Social engineering of users or maintainers.
- Denial of service by a homeserver operator against their own users.

## Verifying releases

Every release ships with:

- `SHA256SUMS`: check downloaded artifacts with
  `sha256sum -c SHA256SUMS --ignore-missing`
- GitHub build provenance attestations: verify an artifact with
  `gh attestation verify <artifact> --repo jpzk/iambitchx`
- a CycloneDX SBOM listing the dependencies the binary was built from

If a release artifact fails verification, do not run it, and report it as
described above.

## Safe harbour

We will not pursue or support legal action against anyone doing good-faith
security research on iambitchx that follows this policy: test only against
accounts, devices and homeservers you own or have permission to use, avoid
privacy violations and data destruction, do not degrade service for others,
and give us reasonable time to fix an issue before disclosing it. If in doubt
whether something is acceptable, ask us first.

# MellowMesh Licensing Kit

> **Status: proposal, not yet adopted.** These files define a licensing
> structure for MellowMesh. The live project license is still MIT (see the root
> `LICENSE`) and **nothing here changes that until you deliberately adopt it**
> by following the steps below. **This is not legal advice** — have counsel
> review before you rely on any of it, especially the CLA, the trademark policy,
> and any commercial agreement.

This kit implements the strategy from the "Neutral-Layer Bet" memo: **give away
the core for adoption, capture value on the outer layers, and make sure
companies that build commercial offerings around MellowMesh contribute back** —
via a source-available license, a commercial license, and a trademark.

## The structure at a glance

| Layer | License | Why |
|---|---|---|
| **Hub, SDKs, CLI, protocol** | **Apache-2.0** (`LICENSE-Apache-2.0.txt`) | Maximize adoption and trust; Apache adds an explicit patent grant, which matters for a would-be standard. This is your eyeballs engine — deliberately free, even for commercial use. |
| **Relay + (future) team control-plane** | **FSL-1.1-ALv2** (`LICENSE-FSL-1.1-ALv2.md`) | Free for internal use, research, education, and professional services — but a **Competing Use** (offering it to others as a hosted/managed service) requires a commercial license. Converts to Apache-2.0 two years after each release, so the community keeps its goodwill. |
| **The brand** | **Trademark policy** (`TRADEMARK.md`) | The code is open; the name is not. Even a permissive-code fork can't call itself "MellowMesh" or claim "certified" status without a brand license. |
| **Escape hatch for companies** | **Commercial license** (`COMMERCIAL-LICENSE.md`) | What a company buys to do a Competing Use, embed commercially, get brand rights, or get a warranty. Your primary low-ops revenue in the lean years. |
| **Inbound contributions** | **CLA** (`CLA.md` + `../.github/workflows/cla-assistant.yml`) | Keeps you holding rights broad enough to dual-license. Without it you could never sell commercial licenses to code others contributed. |

### How each kind of company ends up contributing back

- **Individual devs / internal teams** — use everything free (Apache core, FSL
  internal use). They're your funnel, not your revenue. Correct.
- **A company embedding MellowMesh in a product** — Apache core is free; if they
  need the relay/control-plane commercially, they buy a commercial license.
- **A company reselling MellowMesh as a hosted service** — that's a Competing
  Use under the FSL → commercial license, or they wait for the 2-year Apache
  conversion.
- **Anyone using the brand** ("MellowMesh Cloud", "Powered by MellowMesh",
  "certified") — trademark license, regardless of the code license.

## Proposed per-crate license map

Today all nine crates are MIT. Under this structure:

| Crate | Proposed license |
|---|---|
| `mellowmesh-core` | Apache-2.0 |
| `mellowmesh-store` | Apache-2.0 |
| `mellowmesh-daemon` | Apache-2.0 |
| `mellowmesh-client` | Apache-2.0 |
| `mellowmesh-cli` | Apache-2.0 |
| `mellowmesh-connectors` | Apache-2.0 |
| `mellowmesh-wasm` | Apache-2.0 |
| `mellowmesh-bench` | Apache-2.0 |
| `mellowmesh-relay` | **FSL-1.1-ALv2** |
| *future team/org control-plane* | **FSL-1.1-ALv2** (kept in a separate repo you own outright) |

## Adoption steps (when you're ready)

1. **Decide the legal entity and jurisdiction.** Replace every `Yannick Huchard`
   / `[YOUR JURISDICTION]` / contact placeholder in this kit with your final
   entity (e.g. a company if you form one). The copyright holder should be
   consistent across all files.
2. **Set per-crate licenses in `Cargo.toml`.** For the Apache crates use
   `license = "Apache-2.0"`. For `mellowmesh-relay`, use
   `license = "LicenseRef-FSL-1.1-ALv2"` and `license-file = "LICENSE.md"` with a
   copy of the FSL alongside that crate (SPDX has no registered id for FSL yet,
   so a `LicenseRef-` is correct).
3. **Place the license files.** Copy `LICENSE-Apache-2.0.txt` to the repo root as
   `LICENSE` (and add a short `NOTICE`), and copy `LICENSE-FSL-1.1-ALv2.md` into
   the `crates/mellowmesh-relay/` directory. Keep this `licensing/` folder as the
   canonical source.
4. **Add SPDX headers** to source files (optional but tidy):
   `// SPDX-License-Identifier: Apache-2.0` or `LicenseRef-FSL-1.1-ALv2`.
5. **Turn on the CLA gate.** In GitHub → Settings → Actions → General, set
   Workflow permissions to **Read and write**. The workflow stores signatures in
   a `cla-signatures` branch; no secret needed.
6. **Update the README** license section and badges (currently "MIT"). Describe
   the split honestly: Apache core + FSL relay + commercial option.
7. **File the trademark.** Register "MellowMesh" (word mark, and logo if you have
   one) in your primary markets; update `TRADEMARK.md` from ™ to ® once granted.
8. **Have a lawyer review** the CLA, trademark policy, and a real commercial
   agreement template before you sign anyone.

## Honest caveats

- **Relicensing from MIT is fine, with a wrinkle.** You own 100% of the
  copyright (solo), so you can license *future* releases under Apache/FSL freely.
  But code already published under MIT stays available under MIT for those
  versions — you can't retract that. Going forward is what matters.
- **"Open source" wording.** The FSL is **source-available / "fair source,"**
  not OSI-approved open source. Call the relay/control-plane "source-available"
  and reserve "open source" for the Apache core, or critics will (fairly) call
  it out. This is the same trade Sentry, HashiCorp, and Elastic made.
- **A license is only worth enforcement.** Solo, you won't litigate a
  hyperscaler. Realistic capture = honest mid-size companies who license to avoid
  risk, plus the trademark stopping brand misuse cheaply. Price and plan for
  that, not for stopping a determined bad actor.
- **Alternative if you want to capture from the core too.** If getting
  retribution from *any* commercial user (not just resellers) matters more than
  raw core adoption, the stronger lever is to **dual-license the whole project
  AGPL-3.0 + commercial** instead of Apache+FSL. It captures more but trims
  enterprise adoption (many companies ban AGPL). The CLA in this kit already
  supports that route unchanged; you'd only swap the core license.

## Files in this kit

- `LICENSE-Apache-2.0.txt` — canonical Apache-2.0 text (fetched from apache.org) for the core.
- `LICENSE-FSL-1.1-ALv2.md` — Functional Source License for the relay/control-plane, notice filled in.
- `CLA.md` — Contributor License Agreement (preserves dual-licensing).
- `TRADEMARK.md` — brand-use policy and commercial brand licensing.
- `COMMERCIAL-LICENSE.md` — one-page outline for companies that ask.
- `../.github/workflows/cla-assistant.yml` — automated CLA gate for pull requests.

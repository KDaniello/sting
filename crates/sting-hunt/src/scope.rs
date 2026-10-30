//! The published scopes, as data.
//!
//! A bug bounty scope is a permission, and a permission that lives in a chat message cannot be
//! audited. So it lives here: every asset this repository may report on is one entry below,
//! carrying the programme that published it and the address that programme gave. Anything absent
//! from this table is read for calibration and never reported.
//!
//! The entries are copied from the programmes' own published text, never inferred from what a
//! repository happens to contain. That cuts both ways, and the second direction is the one worth
//! stating: a scope removes code as well as granting it, and an entry carries what the programme
//! excluded beside what it named. Widening a path because it is convenient to scan manufactures a
//! permission nobody gave.
//!
//! Where a programme names its assets loosely — "our products and services" — the entry says so
//! through [`Scope::Unpinned`] rather than guessing a repository: a scope that does not resolve to a
//! commit cannot be hunted reproducibly.

use serde::Serialize;

/// Which part of the ecosystem an asset belongs to. Tiers are kept because the programmes differ
/// in what they pay and how fast they answer, and a report is addressed to a programme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    /// The C++ node: Simplex, Validator Node, Full Node, DHT Node.
    Core,
    /// The reference contracts the chain itself runs on.
    StandardContracts,
    /// Operator and integrator tooling.
    Tools,
    /// Third-party protocols, each with its own programme.
    Ecosystem,
    /// Web properties, reported through a platform rather than against a repository.
    Frontend,
}

impl Tier {
    /// How the tier is written in a report.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::StandardContracts => "standard contracts",
            Self::Tools => "tools",
            Self::Ecosystem => "ecosystem",
            Self::Frontend => "frontend",
        }
    }
}

/// The programme that published a scope, and where it published it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Programme {
    /// How the programme names itself.
    pub name: &'static str,
    /// The published scope text this entry was copied from.
    pub url: &'static str,
}

/// How precisely a published scope names the code it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum Scope {
    /// The scope names a repository, and the run pins it to a ref, a set of paths and a set of
    /// exclusions.
    Repository {
        /// Clone address.
        url: &'static str,
        /// Branch or tag the scope refers to. The run records the commit it resolved to, so a
        /// moving branch still yields a report somebody else can reproduce.
        reference: &'static str,
        /// Files and subtrees the scope covers; empty for the whole repository. A programme that
        /// names four files scopes four files, and measuring the folder around them would overstate
        /// the reach.
        paths: &'static [&'static str],
        /// Subtrees the programme's own text removes from the scope. Each one must match something
        /// at the pinned commit: an exclusion that matches nothing is stale, and a stale exclusion
        /// hides the day a project moves code back into scope.
        excluded: &'static [&'static str],
    },
    /// The programme is published and pays, but names products rather than addresses. Nothing
    /// here resolves to a commit.
    Unpinned {
        /// What the programme says in place of an address.
        names: &'static str,
    },
}

/// One asset inside a published scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Target {
    /// Where the asset sits in the ecosystem.
    pub tier: Tier,
    /// What the published scope calls it.
    pub name: &'static str,
    /// Who published the scope.
    pub programme: Programme,
    /// The address the scope gave, or the reason it gave none.
    pub scope: Scope,
}

const TON: Programme = Programme {
    name: "TON security bug bounty",
    url: "https://github.com/ton-blockchain/bug-bounty",
};
const STON_FI: Programme = Programme {
    name: "STON.fi Bug Bounty",
    url: "https://github.com/ston-fi/bug-bounty",
};
const EVAA: Programme = Programme {
    name: "EVAA, HackenProof",
    url: "https://hackenproof.com/programs/evaa",
};
const TORCH: Programme = Programme {
    name: "Torch Finance Bug Bounty",
    url: "https://github.com/torch-core/bug-bounty",
};
const TONKEEPER: Programme = Programme {
    name: "Tonkeeper Bug Bounty",
    url: "https://tonkeeper.com/bug-bounty",
};
const TON_WHALES: Programme = Programme {
    name: "Ton Whales Bounty Program",
    url: "https://tonwhales.com/bounty",
};
const TON_SOCIETY: Programme = Programme {
    name: "TON Society, HackenProof",
    url: "https://hackenproof.com/programs/ton-society",
};

/// What the core programme's text removes from the main repository, as paths at the pinned commit.
///
/// Two of these deserve their reason written down. The programme excludes the `FunC` and the
/// `Fift` compilers by name; it does not mention the Tolk compiler, because the list was written
/// before Tolk existed. Read literally, that would place the Tolk compiler's own test suite — the
/// largest body of Tolk in the ecosystem — inside the scope of a programme that pays for node
/// crashes. The
/// narrow reading is taken instead: a compiler is a compiler, and its tests are not a node. The
/// standard library beside the reference contracts goes out under the programme's own sentence that
/// the other contracts in that folder are examples.
///
/// The programme's list also removes five things that are not directories at this commit: a
/// non-final liteserver, payment channels, a misconfigured validator control interface, crashing
/// `libemulator` with bad input, and Catchain 1.0. An exclusion here is a path, and none of those
/// five resolves to one, so none is listed — each is C++ either way and none changes what this
/// analyser reads. Everything the programme excludes that *is* a path is above, `blockchain-explorer`
/// included: dropping it would count seven files the programme removed by name.
const CORE_EXCLUSIONS: &[&str] = &[
    "storage",
    "tonlib",
    "blockchain-explorer",
    "http",
    "rldp-http-proxy",
    "crypto/func",
    "crypto/fift",
    "tolk",
    "tolk-tester",
    "crypto/smartcont",
];

/// Every asset this repository may report on, and nothing else.
pub const SCOPE: &[Target] = &[
    Target {
        tier: Tier::Core,
        name: "Simplex, Validator Node, Full Node, DHT Node",
        programme: TON,
        scope: Scope::Repository {
            url: "https://github.com/ton-blockchain/ton",
            reference: "master",
            paths: &[],
            excluded: CORE_EXCLUSIONS,
        },
    },
    Target {
        tier: Tier::StandardContracts,
        name: "Network config, elector, simple and v3 wallets",
        programme: TON,
        scope: Scope::Repository {
            url: "https://github.com/ton-blockchain/ton",
            reference: "master",
            paths: &[
                "crypto/smartcont/config-code.fc",
                "crypto/smartcont/elector-code.fc",
                "crypto/smartcont/simple-wallet-code.fc",
                "crypto/smartcont/wallet3-code.fc",
            ],
            excluded: &[],
        },
    },
    Target {
        tier: Tier::StandardContracts,
        name: "Wallet V4 and subscriptions",
        programme: TON,
        // "Wallet plugins (for all versions) are provided as examples and are out of the scope" —
        // the subscription plugin is the plugin, and the programme names the class rather than the
        // path, so the path is written here.
        scope: Scope::Repository {
            url: "https://github.com/ton-blockchain/wallet-contract",
            reference: "main",
            paths: &[],
            excluded: &["func/simple-subscription-plugin.fc"],
        },
    },
    Target {
        tier: Tier::StandardContracts,
        name: "Multisig V2",
        programme: TON,
        scope: Scope::Repository {
            url: "https://github.com/ton-blockchain/multisig-contract-v2",
            reference: "master",
            paths: &[],
            excluded: &[],
        },
    },
    Target {
        tier: Tier::StandardContracts,
        name: "Highload V3",
        programme: TON,
        scope: Scope::Repository {
            url: "https://github.com/ton-blockchain/highload-wallet-contract-v3",
            reference: "main",
            paths: &[],
            excluded: &[],
        },
    },
    Target {
        tier: Tier::StandardContracts,
        name: "Nominator pool",
        programme: TON,
        scope: Scope::Repository {
            url: "https://github.com/ton-blockchain/nominator-pool",
            reference: "main",
            paths: &[],
            excluded: &[],
        },
    },
    Target {
        tier: Tier::StandardContracts,
        name: "Fungible, non-fungible and semi-fungible tokens",
        programme: TON,
        scope: Scope::Repository {
            url: "https://github.com/ton-blockchain/token-contract",
            reference: "main",
            paths: &[],
            excluded: &[],
        },
    },
    Target {
        tier: Tier::StandardContracts,
        name: "TON DNS",
        programme: TON,
        scope: Scope::Repository {
            url: "https://github.com/ton-blockchain/dns-contract",
            reference: "main",
            paths: &[],
            excluded: &[],
        },
    },
    Target {
        tier: Tier::Tools,
        name: "MyTonCtrl validator tools",
        programme: TON,
        scope: Scope::Repository {
            url: "https://github.com/ton-blockchain/mytonctrl",
            reference: "master",
            paths: &[],
            excluded: &[],
        },
    },
    Target {
        tier: Tier::Tools,
        name: "HTTP API",
        programme: TON,
        scope: Scope::Repository {
            url: "https://github.com/toncenter/ton-http-api",
            reference: "master",
            paths: &[],
            excluded: &[],
        },
    },
    Target {
        tier: Tier::Tools,
        name: "Python SDK",
        programme: TON,
        scope: Scope::Repository {
            url: "https://github.com/toncenter/pytonlib",
            reference: "main",
            paths: &[],
            excluded: &[],
        },
    },
    Target {
        tier: Tier::Ecosystem,
        name: "STON.fi DEX v1.0.0",
        programme: STON_FI,
        scope: Scope::Repository {
            url: "https://github.com/ston-fi/dex-core",
            reference: "v1.0.0",
            paths: &[
                "contracts/lp_account.func",
                "contracts/lp_wallet.func",
                "contracts/pool.func",
                "contracts/router.func",
            ],
            excluded: &[],
        },
    },
    Target {
        tier: Tier::Ecosystem,
        name: "EVAA lending contracts",
        programme: EVAA,
        // The programme is live and pays, but its asset list sits behind a platform that refuses an
        // automated read. `evaafi/contracts` is the obvious guess and stays out of this entry for
        // exactly that reason: an address inferred from what an organisation happens to publish is
        // not the address a programme named.
        scope: Scope::Unpinned {
            names: "assets listed on a platform this run cannot read",
        },
    },
    Target {
        tier: Tier::Ecosystem,
        name: "Torch Stable Swap",
        programme: TORCH,
        scope: Scope::Repository {
            url: "https://github.com/torch-core/torch-dex-contract",
            reference: "main",
            paths: &["contracts"],
            excluded: &["contracts/mock", "contracts/imports"],
        },
    },
    Target {
        tier: Tier::Ecosystem,
        name: "Telegram USD, tgUSD",
        programme: TORCH,
        scope: Scope::Repository {
            url: "https://github.com/torch-core/torch-tgusd-contract",
            reference: "main",
            paths: &["contracts"],
            excluded: &[],
        },
    },
    Target {
        tier: Tier::Ecosystem,
        name: "Ton Whales nominator pool",
        programme: TON_WHALES,
        // The published text links `ton-foundation/ton-nominators`, which is the same repository
        // under its former owner: the redirect and the current path resolve `main` to one commit.
        scope: Scope::Repository {
            url: "https://github.com/tonwhales/ton-nominators",
            reference: "main",
            paths: &[],
            excluded: &[],
        },
    },
    Target {
        tier: Tier::Ecosystem,
        name: "Tonkeeper products and services",
        programme: TONKEEPER,
        scope: Scope::Unpinned {
            names: "products and services, no repository named",
        },
    },
    Target {
        tier: Tier::Frontend,
        name: "TON web services",
        programme: TON_SOCIETY,
        scope: Scope::Unpinned {
            names: "sites and frontend, reported through the platform",
        },
    },
];

#[cfg(test)]
mod tests {
    use super::{SCOPE, Scope};

    /// A duplicate entry double-counts a target in the reach table, which is the one number this
    /// crate exists to produce.
    #[test]
    fn no_target_is_listed_twice() {
        let mut seen = Vec::new();
        for target in SCOPE {
            assert!(!seen.contains(&target.name), "{} listed twice", target.name);
            seen.push(target.name);
        }
    }

    /// A path written as absolute, or with a trailing separator, silently measures the whole
    /// repository instead of what the programme named, and overstates the reach.
    #[test]
    fn every_scoped_path_is_relative() {
        for target in SCOPE {
            if let Scope::Repository {
                paths, excluded, ..
            } = target.scope
            {
                for path in paths.iter().chain(excluded.iter()) {
                    assert!(
                        !path.starts_with('/'),
                        "{} is absolute in {}",
                        path,
                        target.name
                    );
                    assert!(
                        !path.ends_with('/'),
                        "{} has a trailing separator in {}",
                        path,
                        target.name
                    );
                    assert!(!path.is_empty(), "{} has an empty path", target.name);
                }
            }
        }
    }
}

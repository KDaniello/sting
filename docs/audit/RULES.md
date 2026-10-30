# Rules of white-hat work

The rules of conduct for looking for defects in somebody else's code. Read **before** any work on a
target. Breaking any of them cancels a finding entirely, however valuable it is: our standing with the
team we want to join is worth more than one report.

The basis is not a general notion of ethical hacking but the concrete requirements of the official
programme ([`SOURCES.md`](./SOURCES.md)) plus the discipline this project already applies to its own
findings.

---

## 1. Legality and authorisation

**We work strictly inside the published programme.** The bounty repository is an explicit invitation
to look and to report. It is also the boundary: what lies outside the programme's scope
([`SCOPE.md`](./SCOPE.md)) is not touched as a target, even where we technically could.

- **Nothing against mainnet.** Not one transaction, not one packet to a real validator or service to
  test a hypothesis. The whole official method is **local**, and that is not an accident: reproducing
  on the live network would be an attack rather than research.
- **Nothing against third-party services.** The programme lists other people's products and says to
  contact them directly. We do not do that on this track at all — it is about the chain's core.
- **No access we do not have.** The programme explicitly excludes findings that require control of a
  local host, of most honest validators, or leaked private keys. We do not report such things, and we
  do not build hypotheses on them either.

---

## 2. Do no harm

- **Every probe runs on an isolated local network or in a local prober.** Never against shared
  infrastructure.
- **The probing node is self-immune.** A direct rule of the official method: if the probing node in a
  mixed-network experiment falls first and that could explain the outcome, the run is void. We do not
  present such a run as a result.
- **We do not publish an exploit before a fix.** The programme requires that maintainers get a
  reasonable opportunity to investigate and ship or coordinate a fix. That is a condition of
  participation rather than a courtesy.
- **We do not scale a proof of concept.** The proof is a minimal reproducible prober rather than a
  working exploit at full strength. Exponential load is demonstrated at the boundary where the trend
  is visible, not by taking a machine down.

---

## 3. Honesty of a finding, before it is sent

This is the discipline the project already applies to its own detectors, carried over to somebody
else's code. A finding passes it **before** it goes to the intake channel.

- **The hypothesis is stated before the probe, with its expected outcome.** Exactly like a null model:
  first "I claim X and expect to see Y", then the run. Fitting the claim to what was seen is
  forbidden.
- **A finding has to survive an attempt to refute it.** Before believing it, we try to **kill** the
  hypothesis: is there a version gate that already closed it; is gas charged somewhere we missed; does
  the path require something a normal network does not have. The official skill calls those durable
  negative facts and demands the same.
- **A negative result is a valid result.** A run showing there is no defect is data rather than wasted
  time. It goes into the [log](./findings/README.md) rather than being discarded, and parameters are
  not twiddled at random in the hope that something fires.
- **No number by eye.** Gas, memory, processor, sequence numbers are measured rather than estimated.
  The build, the version, the exact prober, the exit code and the measured value are recorded.

---

## 4. Disclosure discipline

- **The channel is the official one only.** Core defects go to the intake bot; front-end issues go to
  the separate programme. Nothing into public issues, nothing into social media, nothing into our own
  public repository before a fix.
- **One report, one finding.** Not a bundle of ten hypotheses, some of them raw. Every submission
  passes all the gates of [`GATES.md`](./GATES.md) separately.
- **We respect their right not to consider a report.** The programme explicitly reserves it. We do not
  argue and do not push.
- **We do not claim somebody else's work.** If a hypothesis came from a tool, an audit report or
  somebody's research, that is stated in the finding's record.

---

## 5. What we never do

The list is concrete, because every item is either a direct prohibition of the programme or a way to
lose trust:

- **We do not report known behaviours.** The programme's documentation marks a specific set as not
  defects; the full list is [`SCOPE.md`](./SCOPE.md) §4. Reporting any of them costs credibility
  immediately.
- **We do not report hygiene.** Status-code preferences, missing headers, open ports without an
  attack, generic recommendations — the programme says outright that these are not valued and are
  refused.
- **We do not report a crash of the emulator on bad data.** Explicitly out of scope. And that is
  exactly the substrate our binding runs on, so "we crashed it with malformed input" is not a finding.
- **We do not report debug opcodes or findings that need debug flags.** The official skill requires
  down-ranking those unless a production host enables the same output on untrusted contract data.
- **We do not report against an outdated version.** Only findings on the current one are accepted.
- **We do not present a guess as a confirmation.** A hypothesis without an executable proof is a
  hypothesis, and the log marks it as one rather than as a finding.

---

## 6. Why the rules are these

We want to enter that team's process rather than to collect a one-off payment. A team that refactors
its own code to make it easier to audit, and keeps its own rules file in the repository, is a team
that will value methodical rigour above a loud finding.

So our rules are stricter than the programme's minimum: the programme would refuse a bad report, and
we want every report of ours to raise trust rather than spend it. One rejected known non-defect costs
more than one accepted real defect earns.

---

## 7. What this document does not cover

It does not say what to look for — that is [`PLAYBOOK.md`](./PLAYBOOK.md) — and it does not define the
conditions of a submission, which are [`GATES.md`](./GATES.md). It defines conduct, and conduct is the
part that cannot be recovered once it is broken.

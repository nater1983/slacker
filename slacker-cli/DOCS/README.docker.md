## Docker image

2 minimal, bootstrappable [Slackware64-current container](https://github.com/rizitis/slackware64-current-ci/pkgs/container/slackware64-current-ci) built on [Slackware64-Current-sofiles](https://github.com/rizitis/Slackware64-Current-sofiles)
repository's dependency database. ~40-64MB pull, 31 packages + [slacker](https://github.com/rizitis/slacker).

Found also in [hub.docker.com](https://hub.docker.com/r/rizitis/slackware-slacker) (podman works too).

```
docker pull ghcr.io/rizitis/slackware64-current-ci:slacker-very_mini-testing
docker run --rm -it ghcr.io/rizitis/slackware64-current-ci:slacker-very_mini-testing
```
```
docker pull rizitis/slackware-slacker:slacker-very_mini-testing
```

If you feal brave:
```
docker pull rizitis/slackware-slacker:sbopkg-ci_slacker-very_mini-testing
```
ΝΟΤΕ: sbopkg-ci_slacker-very_mini-testing image, is [WIP: in very early testing status](https://forge.slackware.nl/rizitis/slacker/src/branch/main/containers/README) it include an mimic version of sbopkg commands under /usr/local/sbin/sbopkg-ci and using "ponce" repo for slackware64-current. Usage: `slacker update --yes && slacker install-template sbo_env --yes && sbopkg-ci -r`

`ls /etc/slacker/templates/` list all templates, you may create your own templates there too.

All images ships **without** gnupg and without a system CA store - slacker's TLS
uses rustls with bundled roots, and packages are verified in two stages:

1. `slacker update` - fetches repo metadata **and this repo's depgraph.db**
   (md5-verified over HTTPS).
2. `slacker install gnupg2` - resolve-stock reads the db's precomputed
   `closure` table and pulls exactly the crypto chain (15 packages, not 300).
3. `slacker update gpg` - pins the Slackware signing key (TOFU); flip
   `VERIFY=all` in `/etc/slacker/slacker.conf` for full GPG verification from
   then on.

From there, `slacker install <anything>` works: the closure table knows what
every stock package needs to *run* on a bare system (library-loader chains
only - tool-only deps never cascade). Build script: [`docker/slacker-docker.sh`](docker/slacker-docker.sh).

Wiki-HowTo:  [slacker-docker](https://forge.slackware.nl/rizitis/slacker/wiki/Docker)

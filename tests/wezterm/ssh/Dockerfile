FROM ubuntu:22.04

ARG WEZTERM_DEB_URL=https://github.com/wezterm/wezterm/releases/download/20240203-110809-5046fc22/wezterm-20240203-110809-5046fc22.Ubuntu22.04.deb
ARG WEZTERM_DEB_SHA256=86358dab5794a4fb63f7c91dd68d4fdc3da58faad648a58fc77d2bd51c7b0686

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install --yes --no-install-recommends \
        ca-certificates \
        curl \
        openssh-server \
    && curl --fail --location --show-error "$WEZTERM_DEB_URL" --output /tmp/wezterm.deb \
    && echo "$WEZTERM_DEB_SHA256  /tmp/wezterm.deb" | sha256sum --check --strict \
    && DEBIAN_FRONTEND=noninteractive apt-get install --yes /tmp/wezterm.deb \
    && rm -f /tmp/wezterm.deb \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --shell /bin/sh vtabs \
    && install -d -m 0755 /run/sshd

COPY entrypoint.sh /usr/local/bin/vtabs-e2e-sshd
COPY remote-backend.sh /usr/local/bin/wez-vtabs-e2e
RUN chmod 0755 /usr/local/bin/vtabs-e2e-sshd /usr/local/bin/wez-vtabs-e2e

EXPOSE 22
ENTRYPOINT ["/usr/local/bin/vtabs-e2e-sshd"]

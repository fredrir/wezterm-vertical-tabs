ARG BASE_IMAGE=docker.io/library/archlinux:base
FROM ${BASE_IMAGE}

RUN if command -v pacman >/dev/null; then \
      pacman -Syu --noconfirm openssh libgit2 && pacman -Scc --noconfirm; \
    else \
      apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends openssh-server libgit2-dev util-linux \
        && rm -rf /var/lib/apt/lists/*; \
    fi \
    && useradd --create-home --shell /bin/sh mux \
    && passwd --delete mux \
    && rm -f /etc/ssh/ssh_host_*
COPY entrypoint.sh /usr/local/bin/ssh-mux-fixture
RUN chmod 755 /usr/local/bin/ssh-mux-fixture
EXPOSE 2222
ENTRYPOINT ["/usr/local/bin/ssh-mux-fixture"]

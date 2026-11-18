# A host for ssh-matrix.sh: an sshd that forwards socket files, on the userland the bench binary is built for
# (BASE: an image whose glibc is at least the build machine's).
ARG BASE=ubuntu:24.04
FROM ${BASE}
RUN apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq openssh-server openssh-client procps python3 >/dev/null \
 && rm -rf /var/lib/apt/lists/* && mkdir -p /run/sshd \
 && printf 'PermitRootLogin prohibit-password\nPasswordAuthentication no\nAllowStreamLocalForwarding yes\nAllowTcpForwarding yes\nStreamLocalBindUnlink yes\n' > /etc/ssh/sshd_config.d/bridge.conf

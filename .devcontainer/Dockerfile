FROM rust:slim-bullseye

ENV RUNNING_IN_CONTAINER=1

RUN apt-get update && apt-get install -y \
    iproute2 \
    ethtool \
    procps \
&& rm -rf /var/lib/apt/lists/*

COPY scripts/setup_veth.sh /usr/local/bin/setup_veth.sh

RUN chmod +x /usr/local/bin/setup_veth.sh

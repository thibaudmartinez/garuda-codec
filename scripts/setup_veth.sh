#!/bin/bash

set -o errexit   # Abort on non-zero exit status.
set -o nounset   # Abort on unbound variable.
set -o pipefail  # Don't hide errors within pipes.

# Check that the user is root.
if [[ $EUID -ne 0 ]]; then
   echo "Error: this script must be run as root."
   exit 1
fi

# usage() displays command usage information.
usage() {
    echo "Usage: $0 --device1 <name> --device2 <name>"
    exit 1
}

# Command arguments.
DEVICE_1=""
DEVICE_2=""

# Parse command arguments.
while [[ "$#" -gt 0 ]]; do
    case $1 in
        --device1) DEVICE_1="$2"; shift ;;
        --device2) DEVICE_2="$2"; shift ;;
        *) echo "Unknown parameter passed: $1"; usage ;;
    esac
    shift
done

# Check that mandatory arguments are provided.
if [ -z "$DEVICE_1" ] || [ -z "$DEVICE_2" ]; then
    echo "Error: --device1 and --device2 are required."
    usage
fi

# Create virtual Ethernet devices.
ip link add "$DEVICE_1" type veth peer name "$DEVICE_2"

# Set device RX and TX queues.
ethtool -L "$DEVICE_1" rx 1 tx 1
ethtool -L "$DEVICE_2" rx 1 tx 1

# Disable IPv6 on network interfaces (not possible from a container).
if [ -z "${RUNNING_IN_CONTAINER:-}" ]; then
    sysctl --quiet -w net.ipv6.conf."$DEVICE_1".disable_ipv6=1
    sysctl --quiet -w net.ipv6.conf."$DEVICE_2".disable_ipv6=1
fi

# Throttle outgoing traffic on device 1, and drop 10% of outgoing packets.
tc qdisc add dev "$DEVICE_1" root handle 1: tbf rate 100mbit burst 32kbit latency 400ms
tc qdisc add dev "$DEVICE_1" parent 1:1 handle 10: netem loss 10%

# Disable outgoing traffic on device 2.
tc qdisc add dev "$DEVICE_2" root netem loss 100%

# Enable network interfaces.
ip link set "$DEVICE_1" up
ip link set "$DEVICE_2" up

# Display network interface details.
echo "Virtual Ethernet devices set up:"
ip addr show "$DEVICE_2"
ip addr show "$DEVICE_1"
